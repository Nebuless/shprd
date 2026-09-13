//! HTTP surface for the native host.

use crate::{auth::Auth, connections, herdr};
use axum::{
    Json, Router,
    extract::{
        Request, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::json;
use shprd_connections::{ConnectionId, Manager, Profile, ProfileService, RpcRoute, resolve_rpc};
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};
use tokio::{
    sync::{Mutex, mpsc},
    task::{JoinHandle, JoinSet},
};
use tower_http::services::ServeDir;

#[cfg(test)]
mod tests {
    use super::*;
    use shprd_connections::{Runtime, RuntimeContext, RuntimeFuture, SocketPaths};

    struct Ready;
    impl Runtime for Ready {
        fn start<'a>(&'a self, _: &'a RuntimeContext) -> RuntimeFuture<'a, SocketPaths> {
            Box::pin(async {
                Ok(SocketPaths {
                    control: "/fixture/control".into(),
                    render: "/fixture/render".into(),
                })
            })
        }
        fn stop(&self) -> RuntimeFuture<'_, ()> {
            Box::pin(async { Ok(()) })
        }
    }

    #[tokio::test]
    async fn queued_payloads_lose_retired_data_at_publication()
    -> Result<(), Box<dyn std::error::Error>> {
        // Given queued reply and event from a ready runtime.
        let profile = Profile::legacy("/fixture/control", "/fixture/render")?;
        let id = profile.id().clone();
        let manager = Manager::new(id.clone());
        manager.register(profile, Arc::new(|_| Ok(Arc::new(Ready))))?;
        manager.connect(&id).await?;
        let lease = manager.lease(&id)?;
        let (send, mut received) = mpsc::channel(2);
        for payload in [
            json!({"id":"pending","result":{"private":"retired"}}),
            json!({"event":"agent_control.event","data":{"private":"retired"}}),
        ] {
            send.send(Outgoing {
                payload,
                lease: Some(lease.clone()),
            })
            .await
            .map_err(|_| "closed queue")?;
        }
        // When retirement happens after enqueue but before the socket publisher reads.
        manager.disconnect(&id).await?;
        // Then no retired data leaves; the outstanding request receives one error.
        let reply = received.try_recv()?.publish().ok_or("missing reply")?;
        assert_eq!(reply["id"], "pending");
        assert!(reply.get("error").is_some());
        assert!(reply.get("result").is_none());
        assert!(received.try_recv()?.publish().is_none());
        Ok(())
    }
}

struct Outgoing {
    payload: serde_json::Value,
    lease: Option<shprd_connections::Lease>,
}

impl Outgoing {
    fn publish(self) -> Option<serde_json::Value> {
        if let Some(lease) = &self.lease {
            if !lease.is_current() {
                return self.payload.get("id").map(|id| json!({
                    "id":id,"connection_id":lease.connection_id,"connection_generation":lease.generation(),
                    "error":{"message":"connection changed during request"}
                }));
            }
        }
        Some(self.payload)
    }
}

async fn send_outgoing(socket: &mut WebSocket, outgoing: Outgoing) -> Result<(), axum::Error> {
    use futures_util::{Sink, SinkExt};
    use std::pin::Pin;
    // Check retirement after backpressure clears, immediately before handing bytes to the sink.
    std::future::poll_fn(|cx| Pin::new(&mut *socket).poll_ready(cx)).await?;
    if let Some(payload) = outgoing.publish() {
        Pin::new(&mut *socket).start_send(Message::Text(payload.to_string().into()))?;
        socket.flush().await?;
    }
    Ok(())
}

struct Host {
    socket: PathBuf,
    auth: Auth,
    attachments: Option<PathBuf>,
    profiles: Option<tokio::sync::RwLock<ProfileService>>,
    manager: Option<Arc<Manager>>,
}

pub fn router() -> axum::Router {
    axum::Router::new()
        .route("/health", axum::routing::get(|| async { "Ok" }))
        .route("/healthz", axum::routing::get(|| async { "Ok" }))
}

pub fn configured_router(socket: PathBuf, public_dir: PathBuf, auth: Auth) -> Router {
    configured_router_with_attachments(
        socket,
        public_dir,
        auth,
        shprd_agent::default_directory().ok(),
    )
}

pub fn configured_router_with_attachments(
    socket: PathBuf,
    public_dir: PathBuf,
    auth: Auth,
    attachments: Option<PathBuf>,
) -> Router {
    build_router(socket, public_dir, auth, attachments, None, None)
}

pub fn configured_router_with_profiles(
    socket: PathBuf,
    public_dir: PathBuf,
    auth: Auth,
    attachments: Option<PathBuf>,
    profiles: ProfileService,
    manager: Arc<Manager>,
) -> Router {
    build_router(
        socket,
        public_dir,
        auth,
        attachments,
        Some(profiles),
        Some(manager),
    )
}

fn build_router(
    socket: PathBuf,
    public_dir: PathBuf,
    auth: Auth,
    attachments: Option<PathBuf>,
    profiles: Option<ProfileService>,
    manager: Option<Arc<Manager>>,
) -> Router {
    let state = Arc::new(Host {
        socket,
        auth,
        attachments,
        profiles: profiles.map(tokio::sync::RwLock::new),
        manager,
    });
    let protected = Router::new()
        .route("/ws", get(websocket))
        .route("/api/health", get(health))
        .route("/api/herdr-info", get(herdr_info))
        .route(
            "/api/connections/{connection_id}/herdr-info",
            get(herdr_info),
        )
        .fallback_service(ServeDir::new(public_dir))
        .layer(middleware::from_fn_with_state(
            Arc::clone(&state),
            authenticate,
        ));
    protected
        .route("/api/login", post(login))
        .route("/login", get(|| async { axum::response::Html(LOGIN) }))
        .with_state(state)
        .merge(router())
}

async fn authenticate(State(state): State<Arc<Host>>, request: Request, next: Next) -> Response {
    let cookie = request
        .headers()
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok());
    if !state.auth.authenticated(cookie) {
        if request.method() == Method::GET
            && request
                .headers()
                .get(header::ACCEPT)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.contains("text/html"))
        {
            return (StatusCode::FOUND, [(header::LOCATION, "/login")]).into_response();
        }
        return StatusCode::UNAUTHORIZED.into_response();
    }
    next.run(request).await
}

async fn health(State(state): State<Arc<Host>>) -> Json<serde_json::Value> {
    Json(json!({"ok":true,"version":env!("CARGO_PKG_VERSION"),"socket":state.socket}))
}

async fn herdr_info(State(state): State<Arc<Host>>, request: Request) -> Response {
    let lease = if let Some(manager) = &state.manager {
        let route =
            shprd_connections::parse_http_route(request.uri().path(), request.method().as_str());
        let query: Result<HashMap<String, String>, _> =
            axum::extract::Query::try_from_uri(request.uri()).map(|value| value.0);
        let resolved = route.and_then(|route| {
            let route = route.ok_or_else(|| {
                shprd_connections::Error::Invalid("invalid connection route".into())
            })?;
            let query =
                query.map_err(|_| shprd_connections::Error::Invalid("invalid query".into()))?;
            let generation = shprd_connections::query_generation(
                query.get("connection_generation").map(String::as_str),
            )?;
            manager.resolve(route.connection_id.as_ref(), generation)
        });
        match resolved {
            Ok(lease) => Some(lease),
            Err(error) => {
                let status = match &error {
                    shprd_connections::Error::Routing { status, .. } => {
                        StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_REQUEST)
                    }
                    shprd_connections::Error::Stale => StatusCode::CONFLICT,
                    _ => StatusCode::BAD_REQUEST,
                };
                return (
                    status,
                    Json(json!({"error":shprd_connections::sanitize_error(&error.to_string())})),
                )
                    .into_response();
            }
        }
    } else {
        None
    };
    let path = lease
        .as_ref()
        .map(|lease| &lease.paths.control)
        .unwrap_or(&state.socket);
    let result = herdr::call(path, "ping", &json!({}), Duration::from_secs(8)).await;
    if lease.as_ref().is_some_and(|lease| !lease.is_current()) {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error":"connection changed during request"})),
        )
            .into_response();
    }
    let mut response = match result {
        Ok(info) => Json(json!({"version":info.get("version"),"protocol":info.get("protocol")}))
            .into_response(),
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":shprd_connections::sanitize_error(&error.to_string())})),
        )
            .into_response(),
    };
    if let Some(lease) = lease {
        for (name, value) in shprd_connections::response_headers(&lease) {
            if let (Ok(name), Ok(value)) = (
                axum::http::HeaderName::try_from(name),
                axum::http::HeaderValue::try_from(value),
            ) {
                response.headers_mut().insert(name, value);
            }
        }
    }
    response
}

#[derive(Deserialize)]
struct Login {
    password: String,
}

async fn login(State(state): State<Arc<Host>>, request: Request) -> Response {
    if !state.auth.required() {
        return Json(json!({"ok":true,"note":"auth not required"})).into_response();
    }
    let bytes = match axum::body::to_bytes(request.into_body(), 16 * 1024).await {
        Ok(bytes) => bytes,
        Err(_) => return StatusCode::PAYLOAD_TOO_LARGE.into_response(),
    };
    let input: Login = match serde_json::from_slice(&bytes) {
        Ok(input) => input,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error":"bad request"})),
            )
                .into_response();
        }
    };
    match state.auth.login(&input.password) {
        Ok(Some(cookie)) => {
            ([(header::SET_COOKIE, cookie)], Json(json!({"ok":true}))).into_response()
        }
        Ok(None) => (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"wrong password"})),
        )
            .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn websocket(State(state): State<Arc<Host>>, upgrade: WebSocketUpgrade) -> Response {
    upgrade
        .max_message_size(1024 * 1024)
        .on_upgrade(|socket| websocket_session(state, socket))
}

async fn websocket_session(state: Arc<Host>, mut socket: WebSocket) {
    let default_id = match &state.manager {
        Some(manager) => match manager.default_id() {
            Ok(id) => id.as_str().to_owned(),
            Err(_) => return,
        },
        None => "legacy-default".to_owned(),
    };
    let hello = json!({"hello":true,"socket":state.socket,"bridge_protocol_version":2,"default_connection_id":default_id, "capabilities":{"connection_id":true,"connection_scoped_http":false,"connection_runtime_generation":true,"native_agents":state.attachments.is_some()}});
    if socket
        .send(Message::Text(hello.to_string().into()))
        .await
        .is_err()
    {
        return;
    }
    let mut requests = JoinSet::new();
    let mut mutations = JoinSet::new();
    let subscriptions = Arc::new(Mutex::new(HashMap::<String, JoinHandle<()>>::new()));
    let (events, mut received) = mpsc::channel::<Outgoing>(128);
    loop {
        tokio::select! {
            message = socket.recv() => {
                match message {
                    Some(Ok(Message::Text(text))) => {
                        if requests.len() + mutations.len() >= 128 {
                            let _ = socket.send(Message::Close(None)).await;
                            break;
                        }
                        let state = Arc::clone(&state);
                        let subscriptions = Arc::clone(&subscriptions);
                        let events = events.clone();
                        let request = serde_json::from_str::<serde_json::Value>(&text);
                        let durable = request.as_ref().ok().and_then(|request| request.get("method")).and_then(serde_json::Value::as_str).is_some_and(|method| matches!(method,
                            "connections.create" | "connections.update" | "connections.remove" | "connections.set_default" | "connections.connect" | "connections.disconnect"));
                        let tasks = if durable { &mut mutations } else { &mut requests };
                        tasks.spawn(async move {
                            let mut lease = None;
                            let payload = match request {
                                Ok(request) => rpc(&state, &request, &subscriptions, &events, &mut lease).await,
                                Err(_) => json!({"id":null,"error":{"message":"bad json"}}),
                            };
                            Outgoing {payload, lease}
                        });
                    }
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    Some(Ok(Message::Binary(_) | Message::Ping(_) | Message::Pong(_))) => {}
                }
            }
            result = requests.join_next(), if !requests.is_empty() => {
                match result {
                    Some(Ok(result)) => {
                        if send_outgoing(&mut socket, result).await.is_err() {break;}
                    }
                    Some(Err(error)) => {eprintln!("WebSocket request task: {error}");break;}
                    None => {}
                }
            }
            Some(event) = received.recv() => {
                if send_outgoing(&mut socket, event).await.is_err() { break; }
            }
            result = mutations.join_next(), if !mutations.is_empty() => {
                match result {
                    Some(Ok(result)) => {
                        if send_outgoing(&mut socket, result).await.is_err() {break;}
                    }
                    Some(Err(error)) => {eprintln!("WebSocket mutation task: {error}");break;}
                    None => {}
                }
            }
        }
    }
    drop(socket);
    requests.shutdown().await;
    let mut subscriptions = subscriptions.lock().await;
    for (_, task) in subscriptions.drain() {
        task.abort();
        let _ = task.await;
    }
    drop(subscriptions);
    // A disconnected requester discards replies, not an in-flight commit or rollback.
    while let Some(result) = mutations.join_next().await {
        if let Err(error) = result {
            eprintln!("WebSocket mutation cleanup: {error}");
        }
    }
}

async fn profile_rpc(
    state: &Host,
    method: &str,
    params: &serde_json::Value,
) -> Result<serde_json::Value, shprd_connections::Error> {
    let profiles = state.profiles.as_ref().ok_or_else(|| {
        shprd_connections::Error::Invalid("connection profiles unavailable".into())
    })?;
    let manager = state.manager.as_ref().ok_or_else(|| {
        shprd_connections::Error::Invalid("connection manager unavailable".into())
    })?;
    let id = || {
        ConnectionId::parse(
            params
                .get("id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(""),
        )
    };
    let profile = || Profile::from_value(params.get("profile").unwrap_or(params).clone());
    match method {
        "connections.list" | "bridge.status" => Ok(
            json!({"default_connection_id":manager.default_id()?,"connections":profiles.read().await.list()?}),
        ),
        "connections.create" => profiles.write().await.create(profile()?).await,
        "connections.update" => {
            profiles
                .write()
                .await
                .update(&id()?, profile()?, connections::probe)
                .await
        }
        "connections.remove" => profiles.write().await.remove(&id()?).await,
        "connections.set_default" => profiles.write().await.set_default(&id()?),
        "connections.connect" => {
            let id = id()?;
            manager.connect(&id).await?;
            profiles.read().await.item(&id)
        }
        "connections.disconnect" => {
            let id = id()?;
            manager.disconnect(&id).await?;
            profiles.read().await.item(&id)
        }
        "connections.test" => Ok(serde_json::to_value(
            profiles
                .read()
                .await
                .test(&profile()?, connections::probe)
                .await?,
        )?),
        _ => Err(shprd_connections::Error::Invalid(
            "unknown bridge method".into(),
        )),
    }
}

async fn rpc(
    state: &Host,
    request: &serde_json::Value,
    subscriptions: &Mutex<HashMap<String, JoinHandle<()>>>,
    events: &mpsc::Sender<Outgoing>,
    outgoing_lease: &mut Option<shprd_connections::Lease>,
) -> serde_json::Value {
    let id = request.get("id").and_then(serde_json::Value::as_str);
    let Some(method) = request
        .get("method")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
    else {
        return json!({"id":id,"error":{"message":"missing id/method"}});
    };
    if id.is_none_or(str::is_empty) {
        return json!({"id":id,"error":{"message":"missing id/method"}});
    }
    let global = method.starts_with("bridge.") || method.starts_with("connections.");
    if global
        && (request.get("connection_id").is_some()
            || request.get("connection_generation").is_some())
    {
        return json!({"id":id,"error":{"message":"bridge-global method must not include connection identity"}});
    }
    if method == "bridge.ping" {
        return json!({"id":id,"result":{"ok":true}});
    }
    if method == "connections.list" || method == "bridge.status" {
        if state.profiles.is_some() {
            return match profile_rpc(state, method, &json!({})).await {
                Ok(result) => json!({"id":id,"result":result}),
                Err(error) => {
                    json!({"id":id,"error":{"message":shprd_connections::sanitize_error(&error.to_string())}})
                }
            };
        }
        let ping = herdr::call(&state.socket, "ping", &json!({}), Duration::from_secs(8)).await;
        let mut connection = json!({"id":"legacy-default","label":"Default","source":"legacy-config","is_default":true,"generation":1,"read_only":true,"auto_connect":true});
        match ping {
            Ok(info)
                if info
                    .get("protocol")
                    .and_then(serde_json::Value::as_u64)
                    .is_some_and(|p| (14..=20).contains(&p) || p == 22) =>
            {
                connection["state"] = json!("ready")
            }
            Ok(_) => {
                connection["state"] = json!("error");
                connection["error"] = json!({"message":"unsupported Herdr protocol"});
            }
            Err(error) => {
                connection["state"] = json!("error");
                connection["error"] = json!({"message":error.to_string()});
            }
        }
        return json!({"id":id,"result":{"default_connection_id":"legacy-default","connections":[connection]}});
    }
    if let Some(value) = request.get("params") {
        if !value.is_object() {
            return json!({"id":id,"error":{"message":"invalid params"}});
        }
    }
    if global {
        if state.profiles.is_some() {
            return match profile_rpc(state, method, request.get("params").unwrap_or(&json!({})))
                .await
            {
                Ok(result) => json!({"id":id,"result":result}),
                Err(error) => {
                    json!({"id":id,"error":{"message":shprd_connections::sanitize_error(&error.to_string())}})
                }
            };
        }
        return json!({"id":id,"error":{"message":"unknown bridge method"}});
    }
    if let Some(manager) = &state.manager {
        let lease = match resolve_rpc(manager, request) {
            Ok(RpcRoute::Connection(lease)) => lease,
            Ok(RpcRoute::Bridge) => {
                return json!({"id":id,"error":{"message":"unknown bridge method"}});
            }
            Err(error) => {
                return json!({"id":id,"connection_id":request.get("connection_id"),"error":{"message":shprd_connections::sanitize_error(&error.to_string())}});
            }
        };
        let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
        *outgoing_lease = Some(lease.clone());
        let result = if method.starts_with("agent_control.") {
            agent_control(state, method, &params, subscriptions, events, Some(&lease)).await
        } else {
            herdr::call(
                &lease.paths.control,
                method,
                &params,
                Duration::from_secs(8),
            )
            .await
            .map_err(|error| error.to_string())
        };
        let result = if lease.is_current() {
            result
        } else {
            Err("connection changed during request".into())
        };
        return match result {
            Ok(result) => {
                json!({"id":id,"connection_id":lease.connection_id,"connection_generation":lease.generation(),"result":result})
            }
            Err(error) => {
                json!({"id":id,"connection_id":lease.connection_id,"connection_generation":lease.generation(),"error":{"message":shprd_connections::sanitize_error(&error)}})
            }
        };
    }
    if request
        .get("connection_id")
        .is_some_and(|value| value.as_str() != Some("legacy-default"))
    {
        return json!({"id":id,"error":{"message":"unknown connection"}});
    }
    if request
        .get("connection_generation")
        .is_some_and(|value| value.as_u64() != Some(1))
    {
        return json!({"id":id,"connection_id":"legacy-default","error":{"message":"connection generation changed"}});
    }
    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
    if method.starts_with("agent_control.") {
        let result = agent_control(state, method, &params, subscriptions, events, None).await;
        return match result {
            Ok(result) => {
                json!({"id":id,"connection_id":"legacy-default","connection_generation":1,"result":result})
            }
            Err(error) => {
                json!({"id":id,"connection_id":"legacy-default","connection_generation":1,"error":{"message":error}})
            }
        };
    }
    match herdr::call(&state.socket, method, &params, Duration::from_secs(8)).await {
        Ok(result) => {
            json!({"id":id,"connection_id":"legacy-default","connection_generation":1,"result":result})
        }
        Err(error) => {
            json!({"id":id,"connection_id":"legacy-default","connection_generation":1,"error":{"message":error.to_string()}})
        }
    }
}

async fn agent_control(
    state: &Host,
    method: &str,
    params: &serde_json::Value,
    subscriptions: &Mutex<HashMap<String, JoinHandle<()>>>,
    events: &mpsc::Sender<Outgoing>,
    lease: Option<&shprd_connections::Lease>,
) -> Result<serde_json::Value, String> {
    if let (Some(lease), Some(profiles)) = (lease, &state.profiles) {
        let profiles = profiles.read().await;
        let profile = profiles
            .profile(&lease.connection_id)
            .map_err(|error| error.to_string())?;
        if !matches!(
            profile.transport(),
            shprd_connections::Transport::Local { .. }
        ) {
            return Err("agent attachments are unavailable for remote connections".into());
        }
        lease.check().map_err(|error| error.to_string())?;
    }
    let directory = state
        .attachments
        .as_ref()
        .ok_or("agent attachments are unavailable")?;
    if method == "agent_control.list" {
        return shprd_agent::list(directory)
            .await
            .map_err(|error| error.to_string());
    }
    let id = params
        .get("session_id")
        .and_then(serde_json::Value::as_str)
        .filter(|id| !id.is_empty() && id.len() <= 256)
        .ok_or("invalid agent session_id")?;
    let connection_id = lease
        .map(|lease| lease.connection_id.as_str())
        .unwrap_or("legacy-default");
    let generation = lease.map(shprd_connections::Lease::generation).unwrap_or(1);
    let key = format!("{connection_id}/{generation}/{id}");
    match method {
        "agent_control.request" => {
            let command: shprd_agent::Command = serde_json::from_value(
                params
                    .get("command")
                    .cloned()
                    .ok_or("missing agent command")?,
            )
            .map_err(|_| "invalid agent command")?;
            shprd_agent::request(directory, id, &command, |_| {})
                .await
                .map_err(|error| error.to_string())
        }
        "agent_control.subscribe" => {
            let mut subscriptions = subscriptions.lock().await;
            subscriptions.retain(|_, task| !task.is_finished());
            if subscriptions
                .get(&key)
                .is_some_and(|task| !task.is_finished())
            {
                return Ok(json!({"ok":true}));
            }
            if subscriptions.len() >= 16 && !subscriptions.contains_key(&key) {
                return Err("too many agent subscriptions".into());
            }
            let mut attachment = shprd_agent::Attachment::connect(directory, id)
                .await
                .map_err(|error| error.to_string())?;
            let events = events.clone();
            let session_id = id.to_owned();
            let connection_id = connection_id.to_owned();
            let lease = lease.cloned();
            let task = tokio::spawn(async move {
                let cancelled = async {
                    match &lease {
                        Some(lease) => lease.cancelled().await,
                        None => std::future::pending().await,
                    }
                };
                tokio::pin!(cancelled);
                loop {
                    let next = tokio::select! {
                        biased;
                        _ = &mut cancelled => break,
                        event = attachment.next_event() => event,
                    };
                    let (data, lost) = match next {
                        Ok(data) => (data, false),
                        Err(_) => (
                            json!({"agent_event":{"session_id":session_id,"event":{"type":"attachment_lost"}}}),
                            true,
                        ),
                    };
                    if lease.as_ref().is_some_and(|lease| !lease.is_current()) {
                        break;
                    }
                    let outgoing = Outgoing {
                        payload: json!({"connection_id":connection_id,"connection_generation":generation,"event":"agent_control.event","data":data}),
                        lease: lease.clone(),
                    };
                    let sent = tokio::select! {
                        biased;
                        _ = &mut cancelled => break,
                        sent = events.send(outgoing) => sent,
                    };
                    if sent.is_err() || lost {
                        break;
                    }
                }
            });
            subscriptions.insert(key, task);
            Ok(json!({"ok":true}))
        }
        "agent_control.unsubscribe" => {
            if let Some(task) = subscriptions.lock().await.remove(&key) {
                task.abort();
                let _ = task.await;
            }
            Ok(json!({"ok":true}))
        }
        _ => Err("unknown agent control method".into()),
    }
}

const LOGIN: &str = r#"<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>SHPRD login</title><style>body{font:16px system-ui;background:#0f1115;color:#e6e8ee;display:grid;place-items:center;min-height:100dvh;margin:0}form{width:min(320px,85vw)}input,button{box-sizing:border-box;width:100%;padding:12px;margin-top:12px}p{min-height:24px}</style><form><h1>SHPRD</h1><label for="password">Password or token</label><input id="password" type="password" autocomplete="current-password" required><button>Log in</button><p role="alert"></p></form><script>document.querySelector('form').addEventListener('submit',async event=>{event.preventDefault();const error=document.querySelector('[role=alert]');try{const response=await fetch('/api/login',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({password:document.querySelector('input').value})});if(response.ok){location.href='/'}else{error.textContent='Wrong password or token'}}catch{error.textContent='Connection failed. Try again.'}})</script></html>"#;
