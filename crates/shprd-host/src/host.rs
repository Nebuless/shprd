//! HTTP surface for the native host.

use crate::{auth::Auth, herdr};
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
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};
use tokio::{
    sync::{Mutex, mpsc},
    task::{JoinHandle, JoinSet},
};
use tower_http::services::ServeDir;

struct Host {
    socket: PathBuf,
    auth: Auth,
    attachments: Option<PathBuf>,
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
    let state = Arc::new(Host {
        socket,
        auth,
        attachments,
    });
    let protected = Router::new()
        .route("/ws", get(websocket))
        .route("/api/health", get(health))
        .route("/api/herdr-info", get(herdr_info))
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

async fn herdr_info(State(state): State<Arc<Host>>) -> Response {
    match herdr::call(&state.socket, "ping", &json!({}), Duration::from_secs(8)).await {
        Ok(info) => Json(json!({"version":info.get("version"),"protocol":info.get("protocol")}))
            .into_response(),
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":error.to_string()})),
        )
            .into_response(),
    }
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
    let hello = json!({"hello":true,"socket":state.socket,"bridge_protocol_version":2,"default_connection_id":"legacy-default", "capabilities":{"connection_id":true,"connection_scoped_http":false,"connection_runtime_generation":true,"native_agents":state.attachments.is_some()}});
    if socket
        .send(Message::Text(hello.to_string().into()))
        .await
        .is_err()
    {
        return;
    }
    let mut requests = JoinSet::new();
    let subscriptions = Arc::new(Mutex::new(HashMap::<String, JoinHandle<()>>::new()));
    let (events, mut received) = mpsc::channel::<serde_json::Value>(128);
    loop {
        tokio::select! {
            message = socket.recv() => {
                match message {
                    Some(Ok(Message::Text(text))) => {
                        if requests.len() >= 128 {
                            let _ = socket.send(Message::Close(None)).await;
                            break;
                        }
                        let state = Arc::clone(&state);
                        let subscriptions = Arc::clone(&subscriptions);
                        let events = events.clone();
                        requests.spawn(async move {
                            match serde_json::from_str::<serde_json::Value>(&text) {
                                Ok(request) => rpc(&state, &request, &subscriptions, &events).await,
                                Err(_) => json!({"id":null,"error":{"message":"bad json"}}),
                            }
                        });
                    }
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    Some(Ok(Message::Binary(_) | Message::Ping(_) | Message::Pong(_))) => {}
                }
            }
            result = requests.join_next(), if !requests.is_empty() => {
                match result {
                    Some(Ok(result)) => {
                        if socket.send(Message::Text(result.to_string().into())).await.is_err() {break;}
                    }
                    Some(Err(error)) => {eprintln!("WebSocket request task: {error}");break;}
                    None => {}
                }
            }
            Some(event) = received.recv() => {
                if socket.send(Message::Text(event.to_string().into())).await.is_err() { break; }
            }
        }
    }
    requests.shutdown().await;
    let mut subscriptions = subscriptions.lock().await;
    for (_, task) in subscriptions.drain() {
        task.abort();
        let _ = task.await;
    }
}

async fn rpc(
    state: &Host,
    request: &serde_json::Value,
    subscriptions: &Mutex<HashMap<String, JoinHandle<()>>>,
    events: &mpsc::Sender<serde_json::Value>,
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
        return json!({"id":id,"error":{"message":"unknown bridge method"}});
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
        let result = agent_control(state, method, &params, subscriptions, events).await;
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
    events: &mpsc::Sender<serde_json::Value>,
) -> Result<serde_json::Value, String> {
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
            if subscriptions
                .get(id)
                .is_some_and(|task| !task.is_finished())
            {
                return Ok(json!({"ok":true}));
            }
            if subscriptions.len() >= 16 && !subscriptions.contains_key(id) {
                return Err("too many agent subscriptions".into());
            }
            let mut attachment = shprd_agent::Attachment::connect(directory, id)
                .await
                .map_err(|error| error.to_string())?;
            let events = events.clone();
            let session_id = id.to_owned();
            let task = tokio::spawn(async move {
                loop {
                    let (data, lost) = match attachment.next_event().await {
                        Ok(data) => (data, false),
                        Err(_) => (
                            json!({"agent_event":{"session_id":session_id,"event":{"type":"attachment_lost"}}}),
                            true,
                        ),
                    };
                    if events.send(json!({"connection_id":"legacy-default","connection_generation":1,"event":"agent_control.event","data":data})).await.is_err() || lost { break; }
                }
            });
            subscriptions.insert(id.to_owned(), task);
            Ok(json!({"ok":true}))
        }
        "agent_control.unsubscribe" => {
            if let Some(task) = subscriptions.lock().await.remove(id) {
                task.abort();
                let _ = task.await;
            }
            Ok(json!({"ok":true}))
        }
        _ => Err("unknown agent control method".into()),
    }
}

const LOGIN: &str = r#"<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>SHPRD login</title><style>body{font:16px system-ui;background:#0f1115;color:#e6e8ee;display:grid;place-items:center;min-height:100dvh;margin:0}form{width:min(320px,85vw)}input,button{box-sizing:border-box;width:100%;padding:12px;margin-top:12px}p{min-height:24px}</style><form><h1>SHPRD</h1><label for="password">Password or token</label><input id="password" type="password" autocomplete="current-password" required><button>Log in</button><p role="alert"></p></form><script>document.querySelector('form').addEventListener('submit',async event=>{event.preventDefault();const error=document.querySelector('[role=alert]');try{const response=await fetch('/api/login',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({password:document.querySelector('input').value})});if(response.ok){location.href='/'}else{error.textContent='Wrong password or token'}}catch{error.textContent='Connection failed. Try again.'}})</script></html>"#;
