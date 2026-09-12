//! Stable generation-one Herdr endpoint transport.

use crate::{herdr, surface::Surface, terminal};
use bincode::{Decode, Encode};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::HashMap, path::Path, time::Duration};
use tokio_util::codec::{Framed, LengthDelimitedCodec};

const LIMIT: usize = 32 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Transport(#[from] terminal::Error),
    #[error("endpoint JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid endpoint contract: {0}")]
    Contract(&'static str),
}

#[derive(Debug, Deserialize)]
pub struct Welcome {
    pub generation: u32,
    pub server_version: String,
    snapshot_codec: String,
    surface_codec: String,
    input_codec: String,
    blob_codec: String,
    #[serde(default)]
    pub methods: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

#[derive(Debug)]
pub enum Event {
    Snapshot(Value),
    Reply { id: String, value: Value },
    Clipboard(String),
    ShellError(String),
    Surface,
    Control,
}

pub struct Endpoint {
    wire: Framed<herdr::Socket, LengthDelimitedCodec>,
    welcome: Welcome,
    boot_id: String,
    pending: HashMap<String, Vec<u8>>,
    surface: Option<Surface>,
}

impl Endpoint {
    pub async fn connect(path: &Path, cols: u16, rows: u16) -> Result<Self, Error> {
        tokio::time::timeout(Duration::from_secs(8), async {
            let socket = herdr::connect(path).await.map_err(terminal::Error::from)?;
            let mut wire = LengthDelimitedCodec::builder().little_endian().max_frame_length(LIMIT).new_framed(socket);
            let hello = json!({"generation":1,"cell_width_px":0,"cell_height_px":0,"surface_size":{"cols":cols,"rows":rows},"pixel_mouse":false,"direct_graphics":false,"endpoint_keybindings":false,"mouse_capture":false,"surface_active":true,"snapshot_codecs":["shell.snapshot.v1"],"surface_codecs":["shell.surface.v1"],"input_codecs":["shell.input.semantic.v1"],"blob_codecs":["shell.blob.v1"]});
            let bytes = bincode::encode_to_vec((20_u32,"endpoint.hello.v1",hello.to_string()),bincode::config::standard()).map_err(terminal::Error::from)?;
            wire.send(bytes.into()).await.map_err(terminal::Error::from)?;
            let mut welcome = None;
            let mut boot_id = String::new();
            while welcome.is_none() || boot_id.is_empty() {
                let bytes = wire.next().await.ok_or(terminal::Error::Closed)?.map_err(terminal::Error::from)?;
                let (tag, offset): (u32, usize) = decode(&bytes)?;
                if tag == 0 { return Err(Error::Contract("server predates endpoint generation one")); }
                if tag != 20 { continue; }
                let ((kind, data), _): ((String, String), usize) = decode(&bytes[offset..])?;
                match kind.as_str() {
                    "endpoint.welcome.v1" => {
                        let negotiated: Welcome = serde_json::from_str(&data)?;
                        if negotiated.generation != 1 || negotiated.snapshot_codec != "shell.snapshot.v1" || negotiated.surface_codec != "shell.surface.v1" || negotiated.input_codec != "shell.input.semantic.v1" || negotiated.blob_codec != "shell.blob.v1" {
                            return Err(Error::Contract("unsupported generation or codecs"));
                        }
                        welcome = Some(negotiated);
                    }
                    "shell.snapshot.v1" => {
                        let snapshot: Value = serde_json::from_str(&data)?;
                        boot_id = snapshot.get("boot_id").and_then(Value::as_str).filter(|id| !id.is_empty()).ok_or(Error::Contract("snapshot lacks boot identity"))?.to_owned();
                    }
                    _ => {}
                }
            }
            Ok(Self {wire, welcome:welcome.ok_or(Error::Contract("missing welcome"))?, boot_id, pending:HashMap::new(),surface:None})
        }).await.map_err(|_| terminal::Error::Timeout)?
    }

    pub const fn negotiation(&self) -> &Welcome {
        &self.welcome
    }

    pub const fn surface(&self) -> Option<&Surface> {
        self.surface.as_ref()
    }

    async fn send(&mut self, value: impl Encode) -> Result<(), Error> {
        let bytes = bincode::encode_to_vec(value, bincode::config::standard())
            .map_err(terminal::Error::from)?;
        self.wire
            .send(bytes.into())
            .await
            .map_err(terminal::Error::from)?;
        Ok(())
    }

    pub async fn resize(&mut self, cols: u16, rows: u16) -> Result<(), Error> {
        self.send((12_u32, 0_u16, 0_u16, cols, rows, false)).await
    }

    pub async fn request(&mut self, id: &str, method: &str, params: &Value) -> Result<(), Error> {
        if !self
            .welcome
            .methods
            .iter()
            .any(|advertised| advertised == method)
        {
            return Err(Error::Contract("method not advertised on this socket"));
        }
        if id.is_empty() || self.pending.contains_key(id) || !params.is_object() {
            return Err(Error::Contract("invalid request identity or parameters"));
        }
        let data = json!({"id":id,"method":method,"params":params}).to_string();
        self.send((15_u32, self.boot_id.clone(), data)).await?;
        self.pending.insert(id.to_owned(), Vec::new());
        Ok(())
    }

    /// Cancellation retains partially received frames in the framed transport.
    pub async fn next(&mut self) -> Result<Event, Error> {
        loop {
            let bytes = self
                .wire
                .next()
                .await
                .ok_or(terminal::Error::Closed)?
                .map_err(terminal::Error::from)?;
            let (tag, offset): (u32, usize) = decode(&bytes)?;
            let body = &bytes[offset..];
            match tag {
                20 => {
                    let ((kind, data), _): ((String, String), usize) = decode(body)?;
                    match kind.as_str() {
                        "shell.snapshot.v1" => {
                            let snapshot: Value = serde_json::from_str(&data)?;
                            if snapshot.get("boot_id").and_then(Value::as_str)
                                != Some(self.boot_id.as_str())
                            {
                                return Err(Error::Contract("server boot identity changed"));
                            }
                            return Ok(Event::Snapshot(snapshot));
                        }
                        "endpoint.health.ping.v1"
                            if self
                                .welcome
                                .capabilities
                                .iter()
                                .any(|cap| cap == "health_check") =>
                        {
                            self.send((20_u32, "endpoint.health.pong.v1", data)).await?;
                        }
                        _ => {}
                    }
                    return Ok(Event::Control);
                }
                18 => {
                    let ((boot, id, last, data), _): ((String, String, bool, Vec<u8>), usize) =
                        decode(body)?;
                    if boot != self.boot_id {
                        return Err(Error::Contract("reply from stale server boot"));
                    }
                    let Some(chunks) = self.pending.get_mut(&id) else {
                        continue;
                    };
                    if chunks.len().saturating_add(data.len()) > LIMIT {
                        return Err(Error::Contract("oversized endpoint reply"));
                    }
                    chunks.extend_from_slice(&data);
                    if last {
                        let value = serde_json::from_slice(chunks)?;
                        self.pending.remove(&id);
                        return Ok(Event::Reply { id, value });
                    }
                }
                5 => {
                    let (data, _) = decode(body)?;
                    return Ok(Event::Clipboard(data));
                }
                13 => {
                    let surface = Surface::decode(body)?;
                    if surface.boot_id != self.boot_id {
                        return Err(Error::Contract("surface from stale server boot"));
                    }
                    self.surface = Some(surface);
                    return Ok(Event::Surface);
                }
                19 => {
                    if let Some(surface) = &mut self.surface {
                        if surface.patch(body)? {
                            return Ok(Event::Surface);
                        }
                    }
                }
                15 => {
                    let (message, _) = decode(body)?;
                    return Ok(Event::ShellError(message));
                }
                0 => return Err(Error::Contract("unexpected legacy welcome")),
                _ => {}
            }
        }
    }
}

pub(crate) fn decode<T: Decode<()>>(bytes: &[u8]) -> Result<(T, usize), Error> {
    bincode::decode_from_slice(bytes, bincode::config::standard().with_limit::<LIMIT>())
        .map_err(terminal::Error::from)
        .map_err(Error::from)
}
