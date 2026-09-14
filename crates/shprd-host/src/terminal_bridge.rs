use crate::{endpoint, render, terminal};
use base64::Engine;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use terminal::Command;
use tokio::{
    sync::{Mutex, mpsc, oneshot, watch},
    task::JoinHandle,
};

const MAX_DIMENSION: u64 = 65_535;
const MAX_CLIPBOARD_AGE_MS: u64 = 30_000;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("terminal_id required")]
    MissingTerminal,
    #[error("surface_cols and surface_rows must be integers between 1 and 65535")]
    InvalidSurface,
    #[error("valid terminal cols and rows required")]
    InvalidSize,
    #[error("invalid terminal input")]
    InvalidInput,
    #[error("terminal input required")]
    EmptyInput,
    #[error("terminal scroll requires positive integer lines")]
    InvalidScroll,
    #[error("terminal is not attached")]
    NotAttached,
    #[error("terminal bridge disposed")]
    Disposed,
    #[error("terminal request timed out")]
    Timeout,
    #[error("terminal write failed: {0}")]
    Write(String),
    #[error(transparent)]
    Transport(#[from] terminal::Error),
    #[error(transparent)]
    Render(#[from] render::Error),
    #[error(transparent)]
    Endpoint(#[from] endpoint::Error),
}

#[derive(Debug, Clone)]
pub struct ViewerFrame {
    pub terminal_id: String,
    pub width: u16,
    pub height: u16,
    pub full: bool,
    pub mouse_reporting: Option<bool>,
    pub bytes: Vec<u8>,
}

pub struct TerminalEvent {
    pub payload: Value,
    pub lease: Option<shprd_connections::Lease>,
    active: Option<Arc<AtomicBool>>,
}

impl TerminalEvent {
    pub(crate) fn new(payload: Value, lease: Option<&shprd_connections::Lease>) -> Self {
        let mut payload = payload;
        if let Some(object) = payload.as_object_mut() {
            let (connection_id, generation) = lease.map_or(("legacy-default", 1), |lease| {
                (lease.connection_id.as_str(), lease.generation())
            });
            object.insert("connection_id".into(), json!(connection_id));
            object.insert("connection_generation".into(), json!(generation));
        }
        Self {
            payload,
            lease: lease.cloned(),
            active: None,
        }
    }

    fn for_viewer(mut self, viewer: &Viewer) -> Self {
        self.active = Some(viewer.active.clone());
        self
    }

    pub fn publish(self) -> Option<Value> {
        if self
            .active
            .as_ref()
            .is_some_and(|active| !active.load(Ordering::Acquire))
            || self.lease.as_ref().is_some_and(|lease| !lease.is_current())
        {
            return None;
        }
        Some(self.payload)
    }
}

impl std::ops::Deref for TerminalEvent {
    type Target = Value;

    fn deref(&self) -> &Self::Target {
        &self.payload
    }
}

#[derive(Debug)]
struct Viewer {
    cols: u16,
    rows: u16,
    sender: mpsc::UnboundedSender<TerminalEvent>,
    active: Arc<AtomicBool>,
}

#[derive(Debug)]
struct SharedTerminal {
    commands: mpsc::Sender<terminal::WriteCommand>,
    transport: JoinHandle<()>,
    viewers: HashMap<String, Viewer>,
    input_owner: Option<(String, u64)>,
    mouse_reporting: Option<bool>,
}

impl Drop for SharedTerminal {
    fn drop(&mut self) {
        self.transport.abort();
    }
}

#[derive(Debug)]
pub enum Transport {
    Direct(u32),
    Endpoint {
        pane_id: String,
        endpoint: Box<endpoint::Endpoint>,
    },
}

pub struct AttachRequest {
    pub viewer_id: String,
    pub terminal_id: String,
    pub cols: u64,
    pub rows: u64,
    pub transport: Transport,
    pub surface_cols: Option<u64>,
    pub surface_rows: Option<u64>,
    pub sender: mpsc::UnboundedSender<TerminalEvent>,
}

#[derive(Debug)]
pub struct ScrollRequest {
    pub viewer_id: String,
    pub terminal_id: String,
    pub direction: String,
    pub lines: u64,
    pub column: Option<u64>,
    pub row: Option<u64>,
    pub source: String,
}

pub struct TerminalBridge {
    render_socket: PathBuf,
    lease: Option<shprd_connections::Lease>,
    shared: Arc<Mutex<HashMap<String, SharedTerminal>>>,
    disposed: watch::Sender<bool>,
    generations: Arc<Mutex<HashMap<String, u64>>>,
}

async fn endpoint_run(
    mut endpoint: endpoint::Endpoint,
    pane_id: String,
    mut commands: mpsc::Receiver<terminal::WriteCommand>,
    outputs: mpsc::UnboundedSender<terminal::Output>,
) {
    loop {
        tokio::select! {
            command = commands.recv() => {
                let Some(terminal::WriteCommand { command, reply }) = command else { return; };
                let result = tokio::time::timeout(Duration::from_secs(8), async { match command {
                    Command::Input(bytes) => endpoint.pane_input(&pane_id, &bytes).await,
                    Command::Resize(cols, rows) => endpoint.resize(cols, rows).await,
                    Command::Scroll { direction, lines, .. } => endpoint.pane_scroll(&pane_id, &direction, lines).await,
                }}).await.map_err(|_| "terminal write timed out".to_owned())
                    .and_then(|result| result.map_err(|error| error.to_string()));
                let failed = result.is_err();
                let _ = reply.send(result);
                if failed {
                    let _ = outputs.send(terminal::Output::Closed(Some("endpoint transport failed".to_owned())));
                    return;
                }
            }
            event = endpoint.next() => {
                match event {
                    Ok(endpoint::Event::Surface) => {
                        match endpoint.pane_frame(&pane_id) {
                            Ok(Some((frame, mouse_reporting))) => {
                                if outputs.send(terminal::Output::Mouse { enabled: mouse_reporting, pixels: false }).is_err() {
                                    return;
                                }
                                if outputs.send(terminal::Output::Frame(frame)).is_err() {
                                    return;
                                }
                            }
                            Ok(None) => {}
                            Err(_) => {
                                let _ = outputs.send(terminal::Output::Closed(Some("endpoint surface failed".to_owned())));
                                return;
                            }
                        }
                    }
                    Ok(endpoint::Event::Clipboard(data)) => {
                        if outputs.send(terminal::Output::Clipboard(data)).is_err() {
                            return;
                        }
                    }
                    Ok(endpoint::Event::ShellError(message)) => {
                        let _ = outputs.send(terminal::Output::Closed(Some(message)));
                        return;
                    }
                    Ok(endpoint::Event::Snapshot(_)
                    | endpoint::Event::Reply { .. }
                    | endpoint::Event::Control) => {}
                    Err(_) => {
                        let _ = outputs.send(terminal::Output::Closed(Some("endpoint transport failed".to_owned())));
                        return;
                    }
                }
            }
        }
    }
}

impl TerminalBridge {
    pub fn new(render_socket: PathBuf) -> Self {
        Self::with_lease(render_socket, None)
    }

    pub fn with_lease(render_socket: PathBuf, lease: Option<shprd_connections::Lease>) -> Self {
        Self {
            render_socket,
            lease,
            shared: Arc::new(Mutex::new(HashMap::new())),
            disposed: watch::channel(false).0,
            generations: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub(crate) fn is_current(&self) -> bool {
        self.lease
            .as_ref()
            .is_none_or(shprd_connections::Lease::is_current)
    }

    pub(crate) async fn has_terminal(&self, id: &str) -> bool {
        self.shared.lock().await.contains_key(id)
    }

    pub async fn attach(&self, request: AttachRequest) -> Result<Value, Error> {
        self.bounded(self.attach_inner(request)).await
    }

    async fn bounded<T>(
        &self,
        future: impl std::future::Future<Output = Result<T, Error>>,
    ) -> Result<T, Error> {
        let mut disposed = self.disposed.subscribe();
        if *disposed.borrow() || !self.is_current() {
            return Err(Error::Disposed);
        }
        tokio::select! {
            biased;
            _ = disposed.changed() => Err(Error::Disposed),
            _ = async { match &self.lease {
                Some(lease) => lease.cancelled().await,
                None => std::future::pending().await,
            }} => Err(Error::Disposed),
            result = tokio::time::timeout(Duration::from_secs(8), future) => result.map_err(|_| Error::Timeout)?,
        }
    }

    async fn attach_inner(&self, request: AttachRequest) -> Result<Value, Error> {
        let AttachRequest {
            viewer_id,
            terminal_id,
            cols,
            rows,
            transport,
            surface_cols,
            surface_rows,
            sender,
        } = request;
        let negotiation = match &transport {
            Transport::Endpoint { endpoint, .. } => endpoint.negotiation_value(),
            Transport::Direct(_) => Value::Null,
        };
        self.ensure_live().await?;
        let viewer_id = viewer_id.as_str();
        let terminal_id = terminal_id.as_str();
        if terminal_id.is_empty() {
            return Err(Error::MissingTerminal);
        }
        let (cols, rows) = size(cols, rows)?;
        let surface = match (surface_cols, surface_rows) {
            (None, None) => None,
            (Some(cols), Some(rows)) => Some(size(cols, rows).map_err(|_| Error::InvalidSurface)?),
            _ => return Err(Error::InvalidSurface),
        };
        let mut shared = self.shared.lock().await;
        let session = if let Some(session) = shared.get_mut(terminal_id) {
            session
        } else {
            let (commands, command_receiver) = mpsc::channel(64);
            let (outputs, mut output_receiver) = mpsc::unbounded_channel();
            let shared_ref = Arc::clone(&self.shared);
            let generations_ref = Arc::clone(&self.generations);
            let terminal_id_owned = terminal_id.to_owned();
            let stream_lease = self.lease.clone();
            let stream_generation = {
                let mut generations = self.generations.lock().await;
                let generation = generations.entry(terminal_id_owned.clone()).or_insert(0);
                *generation = generation.saturating_add(1);
                *generation
            };
            let transport_lease = self.lease.clone();
            let mut disposed = self.disposed.subscribe();
            let transport_run: std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> =
                match transport {
                    Transport::Direct(protocol) => {
                        let mut wire = terminal::Terminal::connect(
                            &self.render_socket,
                            protocol,
                            surface.map_or(cols, |size| size.0),
                            surface.map_or(rows, |size| size.1),
                        )
                        .await?;
                        wire.attach(terminal_id, true).await?;
                        Box::pin(async move {
                            if let Err(error) = wire.run(command_receiver, outputs.clone()).await {
                                let _ = outputs.send(terminal::Output::Closed(Some(
                                    shprd_connections::sanitize_error(&error.to_string()),
                                )));
                            }
                        })
                    }
                    Transport::Endpoint {
                        pane_id, endpoint, ..
                    } => {
                        let mut endpoint = *endpoint;
                        endpoint.focus_and_wait(&pane_id).await?;
                        if let Ok(Some((frame, mouse_reporting))) = endpoint.pane_frame(&pane_id) {
                            let _ = outputs.send(terminal::Output::Mouse {
                                enabled: mouse_reporting,
                                pixels: false,
                            });
                            let _ = outputs.send(terminal::Output::Frame(frame));
                        }
                        Box::pin(endpoint_run(endpoint, pane_id, command_receiver, outputs))
                    }
                };
            let transport_task = tokio::spawn(async move {
                tokio::select! {
                    biased;
                    _ = disposed.changed() => {},
                    _ = async { match &transport_lease {
                        Some(lease) => lease.cancelled().await,
                        None => std::future::pending().await,
                    }} => {},
                    _ = transport_run => {},
                }
            });
            tokio::spawn(async move {
                while let Some(output) = output_receiver.recv().await {
                    match output {
                        terminal::Output::Terminal {
                            width,
                            height,
                            full,
                            mouse_reporting,
                            bytes,
                            ..
                        } => {
                            publish_frame_to(
                                &shared_ref,
                                &generations_ref,
                                &terminal_id_owned,
                                stream_generation,
                                width,
                                height,
                                full,
                                mouse_reporting,
                                bytes,
                                stream_lease.as_ref(),
                            )
                            .await;
                            if let Some(enabled) = mouse_reporting {
                                if let Some(session) =
                                    shared_ref.lock().await.get_mut(&terminal_id_owned)
                                {
                                    if generations_ref
                                        .lock()
                                        .await
                                        .get(&terminal_id_owned)
                                        .copied()
                                        == Some(stream_generation)
                                    {
                                        session.mouse_reporting = Some(enabled);
                                    }
                                }
                            }
                        }
                        terminal::Output::Clipboard(data) => {
                            publish_clipboard_to(
                                &shared_ref,
                                &generations_ref,
                                &terminal_id_owned,
                                stream_generation,
                                &data,
                                now_ms(),
                                stream_lease.as_ref(),
                            )
                            .await
                        }
                        terminal::Output::Closed(reason) => {
                            let mut shared = shared_ref.lock().await;
                            let generations = generations_ref.lock().await;
                            if generations.get(&terminal_id_owned).copied()
                                != Some(stream_generation)
                            {
                                continue;
                            }
                            if let Some(session) = shared.get(&terminal_id_owned) {
                                for viewer in session.viewers.values() {
                                    let _ = viewer.sender.send(TerminalEvent::new(
                                        json!({"terminal_closed":{"terminal_id":terminal_id_owned,"reason":reason}}),
                                        stream_lease.as_ref(),
                                    ).for_viewer(viewer));
                                }
                            }
                            shared.remove(&terminal_id_owned);
                        }
                        terminal::Output::Frame(frame) => {
                            publish_structured_frame(
                                &shared_ref,
                                &generations_ref,
                                &terminal_id_owned,
                                stream_generation,
                                &frame,
                                stream_lease.as_ref(),
                            )
                            .await;
                        }
                        terminal::Output::Mouse { enabled, .. } => {
                            let mut shared = shared_ref.lock().await;
                            let generations = generations_ref.lock().await;
                            if generations.get(&terminal_id_owned).copied()
                                == Some(stream_generation)
                            {
                                if let Some(session) = shared.get_mut(&terminal_id_owned) {
                                    session.mouse_reporting = Some(enabled);
                                }
                            }
                        }
                        terminal::Output::Keyboard { .. } => {}
                    }
                }
                let mut shared = shared_ref.lock().await;
                if generations_ref
                    .lock()
                    .await
                    .get(&terminal_id_owned)
                    .copied()
                    == Some(stream_generation)
                {
                    shared.remove(&terminal_id_owned);
                }
            });
            shared
                .entry(terminal_id.to_owned())
                .or_insert(SharedTerminal {
                    commands,
                    transport: transport_task,
                    viewers: HashMap::new(),
                    input_owner: None,
                    mouse_reporting: None,
                })
        };
        if let Some(viewer) = session.viewers.insert(
            viewer_id.to_owned(),
            Viewer {
                cols,
                rows,
                sender,
                active: Arc::new(AtomicBool::new(true)),
            },
        ) {
            viewer.active.store(false, Ordering::Release);
        }
        Ok(if negotiation.is_null() {
            json!({"ok":true})
        } else {
            json!({"ok":true,"endpoint":negotiation})
        })
    }

    pub async fn input(
        &self,
        viewer_id: &str,
        terminal_id: &str,
        data: &str,
        now_ms: u64,
    ) -> Result<(), Error> {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|_| Error::InvalidInput)?;
        if bytes.is_empty() {
            return Err(Error::EmptyInput);
        }
        let commands = {
            let mut shared = self.shared.lock().await;
            let session = shared.get_mut(terminal_id).ok_or(Error::NotAttached)?;
            if !session.viewers.contains_key(viewer_id) {
                return Err(Error::NotAttached);
            }
            session.input_owner = Some((viewer_id.to_owned(), now_ms));
            session.commands.clone()
        };
        self.write(commands, Command::Input(bytes)).await
    }

    pub async fn resize(
        &self,
        viewer_id: &str,
        terminal_id: &str,
        cols: u64,
        rows: u64,
    ) -> Result<(), Error> {
        let (cols, rows) = size(cols, rows)?;
        let commands = {
            let mut shared = self.shared.lock().await;
            let session = shared.get_mut(terminal_id).ok_or(Error::NotAttached)?;
            let viewer = session
                .viewers
                .get_mut(viewer_id)
                .ok_or(Error::NotAttached)?;
            viewer.cols = cols;
            viewer.rows = rows;
            session.commands.clone()
        };
        self.write(commands, Command::Resize(cols, rows)).await
    }

    pub async fn scroll(&self, request: ScrollRequest) -> Result<(), Error> {
        let ScrollRequest {
            viewer_id,
            terminal_id,
            direction,
            lines,
            column,
            row,
            source,
        } = request;
        let viewer_id = viewer_id.as_str();
        let terminal_id = terminal_id.as_str();
        let direction = direction.as_str();
        let source = source.as_str();
        let lines = u16::try_from(lines)
            .ok()
            .filter(|lines| *lines > 0)
            .ok_or(Error::InvalidScroll)?;
        let column = column
            .map(|value| u16::try_from(value).map_err(|_| Error::InvalidScroll))
            .transpose()?;
        let row = row
            .map(|value| u16::try_from(value).map_err(|_| Error::InvalidScroll))
            .transpose()?;
        let commands = {
            let shared = self.shared.lock().await;
            let session = shared.get(terminal_id).ok_or(Error::NotAttached)?;
            if !session.viewers.contains_key(viewer_id) {
                return Err(Error::NotAttached);
            }
            session.commands.clone()
        };
        self.write(
            commands,
            Command::Scroll {
                direction: direction.to_owned(),
                lines,
                column,
                row,
                source: source.to_owned(),
            },
        )
        .await
    }

    async fn write(
        &self,
        commands: mpsc::Sender<terminal::WriteCommand>,
        command: Command,
    ) -> Result<(), Error> {
        self.bounded(async {
            let (reply, received) = oneshot::channel();
            commands
                .send(terminal::WriteCommand { command, reply })
                .await
                .map_err(|_| Error::Disposed)?;
            received
                .await
                .map_err(|_| Error::Disposed)?
                .map_err(Error::Write)
        })
        .await
    }

    pub async fn detach(&self, viewer_id: &str, terminal_id: Option<&str>) {
        self.detach_and_is_empty(viewer_id, terminal_id).await;
    }

    pub(crate) async fn detach_and_is_empty(
        &self,
        viewer_id: &str,
        terminal_id: Option<&str>,
    ) -> bool {
        let mut shared = self.shared.lock().await;
        let ids = terminal_id.map_or_else(
            || shared.keys().cloned().collect(),
            |terminal_id| vec![terminal_id.to_owned()],
        );
        for id in ids {
            let Some(session) = shared.get_mut(&id) else {
                continue;
            };
            if let Some(viewer) = session.viewers.remove(viewer_id) {
                viewer.active.store(false, Ordering::Release);
            }
            if session
                .input_owner
                .as_ref()
                .is_some_and(|owner| owner.0 == viewer_id)
            {
                session.input_owner = None;
            }
            if session.viewers.is_empty() {
                shared.remove(&id);
                let mut generations = self.generations.lock().await;
                let generation = generations.entry(id).or_insert(0);
                *generation = generation.saturating_add(1);
            }
        }
        shared.is_empty()
    }

    pub async fn publish_clipboard(&self, terminal_id: &str, data: &str, now_ms: u64) {
        let shared = self.shared.lock().await;
        let Some(session) = shared.get(terminal_id) else {
            return;
        };
        let Some((owner, input_ms)) = &session.input_owner else {
            return;
        };
        if now_ms.saturating_sub(*input_ms) > MAX_CLIPBOARD_AGE_MS {
            return;
        }
        if let Some(viewer) = session.viewers.get(owner) {
            let _ = viewer.sender.send(
                TerminalEvent::new(
                    json!({
                        "terminal_clipboard":{"terminal_id":terminal_id,"data":data}
                    }),
                    self.lease.as_ref(),
                )
                .for_viewer(viewer),
            );
        }
    }

    pub async fn publish_frame(&self, frame: ViewerFrame) {
        let shared = self.shared.lock().await;
        let Some(session) = shared.get(&frame.terminal_id) else {
            return;
        };
        for viewer in session.viewers.values() {
            let _ = viewer.sender.send(
                TerminalEvent::new(
                    json!({
                        "terminal": {
                            "terminal_id": frame.terminal_id,
                            "width": frame.width,
                            "height": frame.height,
                            "full": frame.full,
                            "bytes": base64::engine::general_purpose::STANDARD.encode(&frame.bytes),
                            "mouse_reporting": frame.mouse_reporting,
                        }
                    }),
                    self.lease.as_ref(),
                )
                .for_viewer(viewer),
            );
        }
    }

    pub async fn dispose(&self) {
        self.disposed.send_replace(true);
        let mut shared = self.shared.lock().await;
        for session in shared.values() {
            for viewer in session.viewers.values() {
                viewer.active.store(false, Ordering::Release);
            }
        }
        shared.clear();
        self.generations.lock().await.clear();
    }

    async fn ensure_live(&self) -> Result<(), Error> {
        if *self.disposed.borrow() || !self.is_current() {
            Err(Error::Disposed)
        } else {
            Ok(())
        }
    }
}

async fn publish_clipboard_to(
    shared: &Arc<Mutex<HashMap<String, SharedTerminal>>>,
    generations: &Arc<Mutex<HashMap<String, u64>>>,
    terminal_id: &str,
    stream_generation: u64,
    data: &str,
    now_ms: u64,
    lease: Option<&shprd_connections::Lease>,
) {
    let shared = shared.lock().await;
    if generations.lock().await.get(terminal_id).copied() != Some(stream_generation) {
        return;
    }
    let Some(session) = shared.get(terminal_id) else {
        return;
    };
    let Some((owner, input_ms)) = &session.input_owner else {
        return;
    };
    if now_ms.saturating_sub(*input_ms) > MAX_CLIPBOARD_AGE_MS {
        return;
    }
    if let Some(viewer) = session.viewers.get(owner) {
        let _ = viewer.sender.send(
            TerminalEvent::new(
                json!({
                    "terminal_clipboard":{"terminal_id":terminal_id,"data":data}
                }),
                lease,
            )
            .for_viewer(viewer),
        );
    }
}

async fn publish_structured_frame(
    shared: &Arc<Mutex<HashMap<String, SharedTerminal>>>,
    generations: &Arc<Mutex<HashMap<String, u64>>>,
    terminal_id: &str,
    stream_generation: u64,
    frame: &render::Frame,
    lease: Option<&shprd_connections::Lease>,
) {
    let shared = shared.lock().await;
    if generations.lock().await.get(terminal_id).copied() != Some(stream_generation) {
        return;
    }
    let Some(session) = shared.get(terminal_id) else {
        return;
    };
    for viewer in session.viewers.values() {
        let width = frame.width.min(viewer.cols);
        let height = frame.height.min(viewer.rows);
        let Ok(bytes) = render::frame_to_ansi(frame, width, height) else {
            continue;
        };
        let mut terminal = json!({
            "terminal_id": terminal_id,
            "width": width,
            "height": height,
            "full": true,
            "bytes": base64::engine::general_purpose::STANDARD.encode(bytes.as_bytes()),
        });
        if let Some(enabled) = session.mouse_reporting {
            terminal["mouse_reporting"] = json!(enabled);
        }
        let _ = viewer
            .sender
            .send(TerminalEvent::new(json!({"terminal": terminal}), lease).for_viewer(viewer));
    }
}

#[allow(clippy::too_many_arguments)]
async fn publish_frame_to(
    shared: &Arc<Mutex<HashMap<String, SharedTerminal>>>,
    generations: &Arc<Mutex<HashMap<String, u64>>>,
    terminal_id: &str,
    stream_generation: u64,
    width: u16,
    height: u16,
    full: bool,
    mouse_reporting: Option<bool>,
    bytes: Vec<u8>,
    lease: Option<&shprd_connections::Lease>,
) {
    let shared = shared.lock().await;
    if generations.lock().await.get(terminal_id).copied() != Some(stream_generation) {
        return;
    }
    let Some(session) = shared.get(terminal_id) else {
        return;
    };
    for viewer in session.viewers.values() {
        let mut terminal = json!({
            "terminal_id": terminal_id,
            "width": width,
            "height": height,
            "full": full,
            "bytes": base64::engine::general_purpose::STANDARD.encode(&bytes),
        });
        if let Some(enabled) = mouse_reporting.or(session.mouse_reporting) {
            terminal["mouse_reporting"] = json!(enabled);
        }
        let _ = viewer
            .sender
            .send(TerminalEvent::new(json!({"terminal": terminal}), lease).for_viewer(viewer));
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            duration.as_millis().try_into().unwrap_or(u64::MAX)
        })
}

fn size(cols: u64, rows: u64) -> Result<(u16, u16), Error> {
    if !(1..=MAX_DIMENSION).contains(&cols) || !(1..=MAX_DIMENSION).contains(&rows) {
        return Err(Error::InvalidSize);
    }
    Ok((
        u16::try_from(cols).map_err(|_| Error::InvalidSize)?,
        u16::try_from(rows).map_err(|_| Error::InvalidSize)?,
    ))
}
