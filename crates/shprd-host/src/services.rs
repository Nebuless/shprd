use axum::{
    Router,
    body::Body,
    extract::{Path, State},
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::StreamExt;
use serde_json::{Value, json};
use shprd_history::{
    Cursor, FileMeta, HistoryService, HostConfig as HistoryHostConfig, PaneMetadata,
};
use shprd_settings::{
    SettingsIdentity, SettingsService, WorkspaceMetadata, WorktreeHookDiscovery,
    workspace_auto_sync_settings_key,
};
use shprd_workspace::{Checkout, Download, HostConfig, WorkspaceService};
use std::{future::Future, path::PathBuf, pin::Pin, sync::Arc};

const MAX_SAFE_GENERATION: u64 = 9_007_199_254_740_991;
pub type HistorySourceFuture = Pin<Box<dyn Future<Output = Result<HistorySource, String>> + Send>>;
pub type HistorySourceCallback = Arc<dyn Fn(Value) -> HistorySourceFuture + Send + Sync>;
pub type HookDiscoveryFuture =
    Pin<Box<dyn Future<Output = Result<Option<WorktreeHookDiscovery>, String>> + Send>>;
pub type HookDiscoveryCallback =
    Arc<dyn Fn(WorkspaceMetadata) -> HookDiscoveryFuture + Send + Sync>;
pub type AutoSyncRunningCallback = Arc<dyn Fn(&str) -> bool + Send + Sync>;
pub type AutoSyncChangedCallback = Arc<dyn Fn(String, bool) + Send + Sync>;

#[derive(Clone)]
pub struct HistorySource {
    pub pane: PaneMetadata,
    pub file: FileMeta,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GenerationIdentity {
    pub connection_id: String,
    pub connection_generation: u64,
}

#[derive(Clone)]
pub struct ServiceState {
    pub identity: GenerationIdentity,
    pub checkout: Checkout,
    pub workspace: Arc<WorkspaceService>,
    pub settings: Arc<SettingsService>,
    pub history: Arc<HistoryService>,
    generation_is_current: Arc<dyn Fn() -> bool + Send + Sync>,
    history_source: Option<HistorySourceCallback>,
    hook_discovery: Option<HookDiscoveryCallback>,
    auto_sync_running: Option<AutoSyncRunningCallback>,
    auto_sync_changed: Option<AutoSyncChangedCallback>,
}

impl ServiceState {
    pub fn local(
        identity: GenerationIdentity,
        checkout: Checkout,
        settings_path: PathBuf,
        generation_is_current: impl Fn() -> bool + Send + Sync + 'static,
    ) -> Result<Self, ServiceError> {
        let settings = SettingsService::new(
            settings_path,
            SettingsIdentity {
                connection_id: Some(identity.connection_id.clone()),
                host: None,
            },
        )
        .map_err(ServiceError::settings)?;
        let workspace =
            WorkspaceService::new(HostConfig::Local).map_err(ServiceError::workspace)?;
        let history = HistoryService::new(HistoryHostConfig::default());
        Ok(Self {
            identity,
            checkout,
            workspace: Arc::new(workspace),
            settings: Arc::new(settings),
            history: Arc::new(history),
            generation_is_current: Arc::new(generation_is_current),
            history_source: None,
            hook_discovery: None,
            auto_sync_running: None,
            auto_sync_changed: None,
        })
    }

    pub fn with_callbacks(
        mut self,
        history_source: HistorySourceCallback,
        hook_discovery: HookDiscoveryCallback,
        auto_sync_running: AutoSyncRunningCallback,
        auto_sync_changed: AutoSyncChangedCallback,
    ) -> Self {
        self.history_source = Some(history_source);
        self.hook_discovery = Some(hook_discovery);
        self.auto_sync_running = Some(auto_sync_running);
        self.auto_sync_changed = Some(auto_sync_changed);
        self
    }

    pub fn remote(
        identity: GenerationIdentity,
        checkout: Checkout,
        settings_path: PathBuf,
        destination: String,
        generation_is_current: impl Fn() -> bool + Send + Sync + 'static,
    ) -> Result<Self, ServiceError> {
        let settings = SettingsService::new(
            settings_path,
            SettingsIdentity {
                connection_id: Some(identity.connection_id.clone()),
                host: Some(destination.clone()),
            },
        )
        .map_err(ServiceError::settings)?;
        let workspace = WorkspaceService::new(HostConfig::Ssh {
            destination: destination.clone(),
        })
        .map_err(ServiceError::workspace)?;
        let history = HistoryService::new(HistoryHostConfig::default());
        Ok(Self {
            identity,
            checkout,
            workspace: Arc::new(workspace),
            settings: Arc::new(settings),
            history: Arc::new(history),
            generation_is_current: Arc::new(generation_is_current),
            history_source: None,
            hook_discovery: None,
            auto_sync_running: None,
            auto_sync_changed: None,
        })
    }

    pub fn is_current(&self) -> bool {
        (self.generation_is_current)()
    }

    fn check_current(&self) -> Result<(), ServiceError> {
        self.is_current().then_some(()).ok_or(ServiceError::Stale)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("{0}")]
    Invalid(String),
    #[error("connection generation changed")]
    Stale,
    #[error("{0}")]
    Workspace(String),
    #[error("{0}")]
    Settings(String),
    #[error("{0}")]
    History(String),
    #[error("invalid request body")]
    Body,
    #[error("native service callback unavailable")]
    Unsupported,
}

impl ServiceError {
    fn workspace(error: shprd_workspace::Error) -> Self {
        match error {
            shprd_workspace::Error::Stale(_) => Self::Stale,
            other => Self::Workspace(other.to_string()),
        }
    }
    fn settings(error: shprd_settings::Error) -> Self {
        match error {
            shprd_settings::Error::Stale => Self::Stale,
            other => Self::Settings(other.to_string()),
        }
    }
    fn history(error: shprd_history::Error) -> Self {
        Self::History(error.to_string())
    }
    fn status(&self) -> StatusCode {
        match self {
            Self::Invalid(_) | Self::Body => StatusCode::BAD_REQUEST,
            Self::Stale => StatusCode::CONFLICT,
            Self::Unsupported => StatusCode::NOT_IMPLEMENTED,
            Self::Workspace(message)
                if message.contains("escaped") || message.contains("invalid") =>
            {
                StatusCode::BAD_REQUEST
            }
            Self::Workspace(_) | Self::Settings(_) | Self::History(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        }
    }
}

impl IntoResponse for ServiceError {
    fn into_response(self) -> Response {
        let status = self.status();
        (
            status,
            axum::Json(json!({"error":{"message":self.to_string()}})),
        )
            .into_response()
    }
}

pub fn service_router(state: ServiceState) -> Router {
    Router::new()
        .route("/api/file/download", get(legacy_download))
        .route("/api/file/upload", post(legacy_upload))
        .route("/api/file/delete", post(legacy_delete))
        .route("/api/agent-session/download", get(legacy_session_download))
        .route("/api/agent-session/atif", get(legacy_session_atif))
        .route(
            "/api/connections/{connection_id}/file/download",
            get(scoped_download),
        )
        .route(
            "/api/connections/{connection_id}/file/upload",
            post(scoped_upload),
        )
        .route(
            "/api/connections/{connection_id}/file/delete",
            post(scoped_delete),
        )
        .route(
            "/api/connections/{connection_id}/agent-session/download",
            get(scoped_session_download),
        )
        .route(
            "/api/connections/{connection_id}/agent-session/atif",
            get(scoped_session_atif),
        )
        .with_state(Arc::new(state))
}

/// Browser WebSocket glue calls this after resolving the connection lease.
pub async fn dispatch_rpc(state: &ServiceState, request: &Value) -> Result<Value, ServiceError> {
    let id = request
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ServiceError::Invalid("missing id/method".into()))?;
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ServiceError::Invalid("missing id/method".into()))?;
    validate_request_identity(state, request)?;
    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
    if !params.is_object() {
        return Err(ServiceError::Invalid("invalid params".into()));
    }
    let result = dispatch(state, method, &params).await?;
    state.check_current()?;
    Ok(json!({
        "id": id,
        "connection_id": state.identity.connection_id,
        "connection_generation": state.identity.connection_generation,
        "result": result,
    }))
}

fn validate_request_identity(state: &ServiceState, request: &Value) -> Result<(), ServiceError> {
    if let Some(value) = request.get("connection_id") {
        let id = value
            .as_str()
            .ok_or_else(|| ServiceError::Invalid("invalid connection_id".into()))?;
        if id != state.identity.connection_id {
            return Err(ServiceError::Stale);
        }
    }
    if let Some(value) = request.get("connection_generation") {
        let generation = value
            .as_u64()
            .filter(|generation| *generation <= MAX_SAFE_GENERATION)
            .ok_or_else(|| ServiceError::Invalid("invalid connection_generation".into()))?;
        if generation != state.identity.connection_generation {
            return Err(ServiceError::Stale);
        }
    }
    state.check_current()
}

async fn dispatch(
    state: &ServiceState,
    method: &str,
    params: &Value,
) -> Result<Value, ServiceError> {
    if method.starts_with("file.") || method.starts_with("git.") {
        state.check_current()?;
        return state
            .workspace
            .dispatch_if_current(&state.checkout, method, params, {
                let state = state.clone();
                move || state.is_current()
            })
            .await
            .map_err(ServiceError::workspace);
    }
    if method == "settings.get" {
        let settings = state
            .settings
            .read_json()
            .await
            .map_err(ServiceError::settings)?;
        return Ok(json!({"settings": settings, "path": state.settings.path().to_string_lossy()}));
    }
    if method == "settings.update_repo" {
        let key = params
            .get("key")
            .and_then(Value::as_str)
            .ok_or_else(|| ServiceError::Invalid("settings.update_repo requires key".into()))?;
        let patch = shprd_settings::RepoSettingsPatch {
            worktree_hooks_enabled: params
                .get("worktree_hooks_enabled")
                .and_then(Value::as_bool),
            custom: params.get("custom").and_then(Value::as_object).cloned(),
        };
        state.check_current()?;
        return serde_json::to_value(
            state
                .settings
                .update_repo_if_current(key, patch, {
                    let state = state.clone();
                    move || state.is_current()
                })
                .await
                .map_err(ServiceError::settings)?,
        )
        .map_err(|error| ServiceError::Settings(error.to_string()));
    }
    if method == "settings.workspace_auto_sync.get" {
        let workspace = workspace_metadata(&state.checkout);
        let key = workspace_auto_sync_settings_key(&state.checkout.path, state.settings.identity())
            .map_err(ServiceError::settings)?;
        let running_callback = state.auto_sync_running_callback()?;
        let running = running_callback(&key);
        return serde_json::to_value(
            state
                .settings
                .workspace_auto_sync(&workspace, running)
                .await
                .map_err(ServiceError::settings)?,
        )
        .map_err(|error| ServiceError::Settings(error.to_string()));
    }
    if method == "settings.workspace_auto_sync.update" {
        let enabled = params
            .get("enabled")
            .and_then(Value::as_bool)
            .ok_or_else(|| {
                ServiceError::Invalid("settings.workspace_auto_sync.update requires enabled".into())
            })?;
        let workspace = workspace_metadata(&state.checkout);
        let key = workspace_auto_sync_settings_key(&state.checkout.path, state.settings.identity())
            .map_err(ServiceError::settings)?;
        let running_callback = state.auto_sync_running_callback()?;
        let changed_callback = state.auto_sync_changed_callback()?;
        let running = running_callback(&key);
        state.check_current()?;
        let value = state
            .settings
            .update_workspace_auto_sync_if_current(&workspace, enabled, running, {
                let state = state.clone();
                move || state.is_current()
            })
            .await
            .map_err(ServiceError::settings)?;
        state.check_current()?;
        changed_callback(value.key.clone(), enabled);
        return serde_json::to_value(value)
            .map_err(|error| ServiceError::Settings(error.to_string()));
    }
    if method == "settings.workspace_auto_sync.list" {
        let running_callback = state.auto_sync_running_callback()?;
        let running_for_settings = |key: &str| running_callback(key);
        let configs = state
            .settings
            .list_workspace_auto_sync(&running_for_settings)
            .await
            .map_err(ServiceError::settings)?;
        return Ok(json!({"configs": configs, "path": state.settings.path().to_string_lossy()}));
    }
    if method == "settings.workspace_auto_sync.update_key" {
        let key = params.get("key").and_then(Value::as_str).ok_or_else(|| {
            ServiceError::Invalid("settings.workspace_auto_sync.update_key requires key".into())
        })?;
        let enabled = params
            .get("enabled")
            .and_then(Value::as_bool)
            .ok_or_else(|| {
                ServiceError::Invalid(
                    "settings.workspace_auto_sync.update_key requires enabled".into(),
                )
            })?;
        let changed_callback = state.auto_sync_changed_callback()?;
        state.check_current()?;
        let entry = state
            .settings
            .update_auto_sync_key_if_current(key, enabled, {
                let state = state.clone();
                move || state.is_current()
            })
            .await
            .map_err(ServiceError::settings)?;
        state.check_current()?;
        changed_callback(key.to_owned(), enabled);
        Ok(
            json!({"key":key,"enabled":entry.enabled,"interval_minutes":entry.interval_minutes,"checkout_path":entry.checkout_path,"host":entry.host,"last_run_at":entry.last_run_at,"last_status":entry.last_status,"last_message":entry.last_message,"last_branch":entry.last_branch}),
        )
    } else if method == "settings.worktree_hooks.get" {
        let workspace = workspace_metadata(&state.checkout);
        let discovery = state.hook_discovery(&workspace).await?;
        serde_json::to_value(
            state
                .settings
                .worktree_hooks_view(&workspace, discovery, None)
                .await
                .map_err(ServiceError::settings)?,
        )
        .map_err(|error| ServiceError::Settings(error.to_string()))
    } else if method == "agent_history.get" || method == "agent_history.entry" {
        history_dispatch(state, method, params).await
    } else if method == "agent_session.get" {
        session_summary_dispatch(state, params).await
    } else {
        Err(ServiceError::Invalid(format!(
            "unknown service method: {method}"
        )))
    }
}

impl ServiceState {
    fn auto_sync_running_callback(&self) -> Result<AutoSyncRunningCallback, ServiceError> {
        self.auto_sync_running
            .clone()
            .ok_or(ServiceError::Unsupported)
    }

    fn auto_sync_changed_callback(&self) -> Result<AutoSyncChangedCallback, ServiceError> {
        self.auto_sync_changed
            .clone()
            .ok_or(ServiceError::Unsupported)
    }

    async fn hook_discovery(
        &self,
        workspace: &WorkspaceMetadata,
    ) -> Result<Option<WorktreeHookDiscovery>, ServiceError> {
        let callback = self
            .hook_discovery
            .as_ref()
            .ok_or(ServiceError::Unsupported)?;
        callback(workspace.clone())
            .await
            .map_err(ServiceError::Settings)
    }
}

fn parse_cursor(value: &Value) -> Result<Cursor, ServiceError> {
    let object = value
        .as_object()
        .ok_or_else(|| ServiceError::Invalid("invalid history cursor".into()))?;
    let epoch = object
        .get("epoch")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ServiceError::Invalid("invalid history cursor".into()))?;
    let revision = object
        .get("revision")
        .and_then(Value::as_u64)
        .ok_or_else(|| ServiceError::Invalid("invalid history cursor".into()))?;
    Ok(Cursor {
        epoch: epoch.into(),
        revision,
    })
}

fn workspace_metadata(checkout: &Checkout) -> shprd_settings::WorkspaceMetadata {
    shprd_settings::WorkspaceMetadata {
        workspace_id: checkout.workspace_id.clone(),
        label: Some(checkout.repo_name.clone()),
        repo_key: Some(checkout.repo_name.clone()),
        repo_root: Some(checkout.path.clone()),
        checkout_path: Some(checkout.path.clone()),
    }
}

async fn session_summary_dispatch(
    state: &ServiceState,
    params: &Value,
) -> Result<Value, ServiceError> {
    state.check_current()?;
    let source = state
        .history_source
        .as_ref()
        .ok_or(ServiceError::Unsupported)?(params.clone())
    .await
    .map_err(ServiceError::History)?;
    state.check_current()?;
    let projection = state
        .history
        .project_text(&source.pane, source.file.clone(), &source.text)
        .map_err(ServiceError::history)?;
    let include_text = params
        .get("include_text")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let include_trajectory = params
        .get("include_trajectory")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let view = state
        .history
        .session_view(
            &projection,
            include_text.then_some(source.text.as_str()),
            include_trajectory,
        )
        .map_err(ServiceError::history)?;
    state.check_current()?;
    Ok(json!({
        "status": "ok",
        "pane_id": source.pane.pane_id,
        "workspace_id": source.pane.workspace_id,
        "agent": source.pane.agent.canonical_name(),
        "path": source.file.path,
        "updated_at": source.file.mtime_ms,
        "stats": view.stats,
        "text": view.text,
        "truncated": view.truncated,
        "trajectory": view.trajectory,
    }))
}

async fn history_dispatch(
    state: &ServiceState,
    method: &str,
    params: &Value,
) -> Result<Value, ServiceError> {
    state.check_current()?;
    let source = state
        .history_source
        .as_ref()
        .ok_or(ServiceError::Unsupported)?(params.clone())
    .await
    .map_err(ServiceError::History)?;
    state.check_current()?;
    if method == "agent_history.get" {
        let cursor = params.get("cursor").map(parse_cursor).transpose()?;
        let (_, update) = state
            .history
            .history(&source.pane, source.file, &source.text, cursor.as_ref())
            .map_err(ServiceError::history)?;
        return serde_json::to_value(update)
            .map_err(|error| ServiceError::History(error.to_string()));
    }
    let id = params
        .get("entry_id")
        .and_then(Value::as_str)
        .ok_or_else(|| ServiceError::Invalid("history entry requires entry_id".into()))?;
    let projection = state
        .history
        .project_text(&source.pane, source.file, &source.text)
        .map_err(ServiceError::history)?;
    serde_json::to_value(
        state
            .history
            .entry(&projection, id)
            .map_err(ServiceError::history)?,
    )
    .map_err(|error| ServiceError::History(error.to_string()))
}

async fn legacy_download(
    State(state): State<Arc<ServiceState>>,
    request: Request<Body>,
) -> Result<Response, ServiceError> {
    http_download(state, request, None).await
}

async fn scoped_download(
    Path(connection_id): Path<String>,
    State(state): State<Arc<ServiceState>>,
    request: Request<Body>,
) -> Result<Response, ServiceError> {
    http_download(state, request, Some(connection_id)).await
}

async fn http_download(
    state: Arc<ServiceState>,
    request: Request<Body>,
    route_connection_id: Option<String>,
) -> Result<Response, ServiceError> {
    let params = http_params(
        &state,
        request.uri().query(),
        route_connection_id.as_deref(),
    )?;
    state.check_current()?;
    let download = state
        .workspace
        .download(&state.checkout, &params)
        .await
        .map_err(ServiceError::workspace)?;
    state.check_current()?;
    response_for_download(download, state)
}

async fn legacy_upload(
    State(state): State<Arc<ServiceState>>,
    request: Request<Body>,
) -> Result<axum::Json<Value>, ServiceError> {
    http_upload(state, request, None).await
}

async fn scoped_upload(
    Path(connection_id): Path<String>,
    State(state): State<Arc<ServiceState>>,
    request: Request<Body>,
) -> Result<axum::Json<Value>, ServiceError> {
    http_upload(state, request, Some(connection_id)).await
}

async fn http_upload(
    state: Arc<ServiceState>,
    request: Request<Body>,
    route_connection_id: Option<String>,
) -> Result<axum::Json<Value>, ServiceError> {
    let (parts, body) = request.into_parts();
    let params = http_params(&state, parts.uri.query(), route_connection_id.as_deref())?;
    state.check_current()?;
    let bytes = axum::body::to_bytes(body, 25 * 1024 * 1024)
        .await
        .map_err(|_| ServiceError::Body)?;
    state.check_current()?;
    let mut reader = std::io::Cursor::new(bytes);
    let current_state = Arc::clone(&state);
    let result = state
        .workspace
        .upload_if_current(&state.checkout, &params, &mut reader, move || {
            current_state.is_current()
        })
        .await
        .map_err(ServiceError::workspace)?;
    state.check_current()?;
    Ok(axum::Json(json!({
        "connection_id": state.identity.connection_id,
        "connection_generation": state.identity.connection_generation,
        "result": result,
    })))
}

async fn legacy_session_download(
    State(state): State<Arc<ServiceState>>,
    request: Request<Body>,
) -> Result<Response, ServiceError> {
    http_session(state, request, None, false).await
}

async fn scoped_session_download(
    Path(connection_id): Path<String>,
    State(state): State<Arc<ServiceState>>,
    request: Request<Body>,
) -> Result<Response, ServiceError> {
    http_session(state, request, Some(connection_id), false).await
}

async fn legacy_session_atif(
    State(state): State<Arc<ServiceState>>,
    request: Request<Body>,
) -> Result<Response, ServiceError> {
    http_session(state, request, None, true).await
}

async fn scoped_session_atif(
    Path(connection_id): Path<String>,
    State(state): State<Arc<ServiceState>>,
    request: Request<Body>,
) -> Result<Response, ServiceError> {
    http_session(state, request, Some(connection_id), true).await
}

async fn http_session(
    state: Arc<ServiceState>,
    request: Request<Body>,
    route_connection_id: Option<String>,
    atif: bool,
) -> Result<Response, ServiceError> {
    let params = http_params(
        &state,
        request.uri().query(),
        route_connection_id.as_deref(),
    )?;
    state.check_current()?;
    let source = state
        .history_source
        .as_ref()
        .ok_or(ServiceError::Unsupported)?(params)
    .await
    .map_err(ServiceError::History)?;
    state.check_current()?;
    let body = if atif {
        let projection = state
            .history
            .project_text(&source.pane, source.file.clone(), &source.text)
            .map_err(ServiceError::history)?;
        let value = state
            .history
            .atif_json(&projection)
            .map_err(ServiceError::history)?;
        format!(
            "{}\n",
            serde_json::to_string_pretty(&value)
                .map_err(|error| ServiceError::History(error.to_string()))?
        )
    } else {
        source.text
    };
    state.check_current()?;
    let content_type = if atif {
        "application/json; charset=utf-8"
    } else {
        "application/x-ndjson; charset=utf-8"
    };
    Response::builder()
        .header("content-type", content_type)
        .header("content-length", body.len())
        .body(Body::from(body))
        .map_err(|error| ServiceError::Invalid(error.to_string()))
}

async fn legacy_delete(
    State(state): State<Arc<ServiceState>>,
    request: Request<Body>,
) -> Result<axum::Json<Value>, ServiceError> {
    http_delete(state, request, None).await
}

async fn scoped_delete(
    Path(connection_id): Path<String>,
    State(state): State<Arc<ServiceState>>,
    request: Request<Body>,
) -> Result<axum::Json<Value>, ServiceError> {
    http_delete(state, request, Some(connection_id)).await
}

async fn http_delete(
    state: Arc<ServiceState>,
    request: Request<Body>,
    route_connection_id: Option<String>,
) -> Result<axum::Json<Value>, ServiceError> {
    let params = http_params(
        &state,
        request.uri().query(),
        route_connection_id.as_deref(),
    )?;
    state.check_current()?;
    let result = state
        .workspace
        .dispatch_if_current(&state.checkout, "file.delete", &params, {
            let state = Arc::clone(&state);
            move || state.is_current()
        })
        .await
        .map_err(ServiceError::workspace)?;
    state.check_current()?;
    Ok(axum::Json(json!({
        "connection_id": state.identity.connection_id,
        "connection_generation": state.identity.connection_generation,
        "result": result,
    })))
}

fn response_for_download(
    download: Download,
    state: Arc<ServiceState>,
) -> Result<Response, ServiceError> {
    let stream = tokio_util::io::ReaderStream::new(download.body).map(move |chunk| {
        if state.is_current() {
            chunk
        } else {
            Err(std::io::Error::other("connection generation changed"))
        }
    });
    let mut response = Response::new(Body::from_stream(stream));
    for (key, value) in download.headers {
        let name = axum::http::header::HeaderName::try_from(key)
            .map_err(|e| ServiceError::Invalid(e.to_string()))?;
        let value = axum::http::HeaderValue::try_from(value)
            .map_err(|e| ServiceError::Invalid(e.to_string()))?;
        response.headers_mut().insert(name, value);
    }
    Ok(response)
}

fn http_params(
    state: &ServiceState,
    query: Option<&str>,
    route_connection_id: Option<&str>,
) -> Result<Value, ServiceError> {
    if route_connection_id.is_some_and(|id| id != state.identity.connection_id) {
        return Err(ServiceError::Stale);
    }
    let mut params = query_params(query.unwrap_or_default())?;
    if params.get("inline").is_some() {
        let inline = parse_inline(&params)?;
        params["inline"] = Value::Bool(inline);
    }
    if let Some(value) = params.get("connection_generation") {
        let generation = value
            .as_str()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|generation| *generation <= MAX_SAFE_GENERATION)
            .ok_or_else(|| ServiceError::Invalid("invalid connection_generation".into()))?;
        if generation != state.identity.connection_generation {
            return Err(ServiceError::Stale);
        }
    }
    Ok(params)
}

fn query_params(query: &str) -> Result<Value, ServiceError> {
    let mut object = serde_json::Map::new();
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        object.insert(form_decode(key)?, Value::String(form_decode(value)?));
    }
    Ok(Value::Object(object))
}

fn form_decode(value: &str) -> Result<String, ServiceError> {
    let mut bytes = Vec::with_capacity(value.len());
    let mut chars = value.bytes();
    while let Some(byte) = chars.next() {
        if byte == b'+' {
            bytes.push(b' ');
            continue;
        }
        if byte != b'%' {
            bytes.push(byte);
            continue;
        }
        let high = chars.next().and_then(|byte| char::from(byte).to_digit(16));
        let low = chars.next().and_then(|byte| char::from(byte).to_digit(16));
        let (Some(high), Some(low)) = (high, low) else {
            return Err(ServiceError::Invalid("invalid URL encoding".into()));
        };
        bytes.push(
            u8::try_from(high * 16 + low)
                .map_err(|_| ServiceError::Invalid("invalid URL encoding".into()))?,
        );
    }
    String::from_utf8(bytes).map_err(|_| ServiceError::Invalid("invalid URL encoding".into()))
}

fn parse_inline(params: &Value) -> Result<bool, ServiceError> {
    match params.get("inline") {
        None => Ok(false),
        Some(Value::String(value)) if value == "true" || value == "1" => Ok(true),
        Some(Value::String(value)) if value == "false" || value == "0" => Ok(false),
        Some(_) => Err(ServiceError::Invalid("invalid inline".into())),
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn state() -> (tempfile::TempDir, ServiceState) {
        state_with_guard(|| true)
    }

    fn state_with_guard<F>(generation_is_current: F) -> (tempfile::TempDir, ServiceState)
    where
        F: Fn() -> bool + Send + Sync + 'static,
    {
        let dir = tempfile::tempdir().expect("fixture dir");
        let settings = dir.path().join("settings.json");
        let checkout = Checkout {
            workspace_id: "workspace-1".into(),
            path: dir.path().to_string_lossy().into_owned(),
            repo_name: "repo".into(),
        };
        let state = ServiceState::local(
            GenerationIdentity {
                connection_id: "local".into(),
                connection_generation: 7,
            },
            checkout,
            settings,
            generation_is_current,
        )
        .expect("state")
        .with_callbacks(
            Arc::new(|_| Box::pin(async { Err("history callback not configured".into()) })),
            Arc::new(|_| Box::pin(async { Err("hook callback not configured".into()) })),
            Arc::new(|_| false),
            Arc::new(|_, _| {}),
        );
        (dir, state)
    }

    #[tokio::test]
    async fn workspace_route_contract() {
        let (_dir, state) = state();
        let value = dispatch_rpc(
            &state,
            &json!({"id":"req-1","method":"file.list","params":{},"connection_id":"local","connection_generation":7}),
        )
        .await
        .expect("response");
        assert_eq!(value["id"], "req-1");
        assert_eq!(value["connection_id"], "local");
        assert_eq!(value["connection_generation"], 7);
        assert!(value["result"]["entries"].is_array());
    }

    #[tokio::test]
    async fn workspace_file_git_contract() {
        let (_dir, state) = state();
        let error = dispatch_rpc(
            &state,
            &json!({"id":"bad","method":"file.read","params":{"path":"../outside"}}),
        )
        .await
        .expect_err("traversal must fail");
        assert!(matches!(
            error,
            ServiceError::Invalid(_) | ServiceError::Workspace(_)
        ));
        let result = dispatch_rpc(
            &state,
            &json!({"id":"status","method":"git.status","params":{}}),
        )
        .await
        .expect("response");
        assert!(result["result"].is_object());
    }

    #[tokio::test]
    async fn settings_history_rpc_contract() {
        let (_dir, state) = state();
        let value = dispatch_rpc(
            &state,
            &json!({"id":"settings","method":"settings.get","params":{}}),
        )
        .await
        .expect("settings response");
        assert!(value["result"]["settings"].is_object());
        let error = dispatch_rpc(
            &state,
            &json!({"id":"stale","method":"file.list","params":{},"connection_id":"local","connection_generation":8}),
        )
        .await
        .expect_err("stale generation must fail");
        assert!(matches!(error, ServiceError::Stale));
    }

    #[tokio::test]
    async fn stale_generation_guard_blocks_publication() {
        let (_dir, state) = state_with_guard(|| false);
        let response = service_router(state)
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/file/download?path=stale.txt&connection_generation=7")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn generation_guard_is_required_for_scoped_state() {
        let (_dir, state) = state_with_guard(|| false);
        let error = dispatch_rpc(
            &state,
            &json!({"id":"stale","method":"file.list","params":{}}),
        )
        .await
        .expect_err("retired scoped state must reject work");
        assert!(matches!(error, ServiceError::Stale));
    }

    #[tokio::test]
    async fn live_router_contract() {
        let (_dir, state) = state();
        std::fs::write(PathBuf::from(&state.checkout.path).join("live.txt"), "live")
            .expect("fixture");
        let response = service_router(state)
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/file/download?path=live.txt&connection_generation=7")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-length"], "4");
        assert_eq!(
            response
                .into_body()
                .collect()
                .await
                .expect("body")
                .to_bytes(),
            "live"
        );
    }

    #[tokio::test]
    async fn query_plus_and_inline_contracts_are_preserved() {
        let dir = tempfile::tempdir().expect("fixture");
        let checkout = Checkout {
            workspace_id: "workspace-1".into(),
            path: dir.path().to_string_lossy().into_owned(),
            repo_name: "repo".into(),
        };
        std::fs::write(dir.path().join("a+b.txt"), "literal-plus").expect("plus fixture");
        std::fs::write(dir.path().join("a b.txt"), "space").expect("space fixture");
        std::fs::write(dir.path().join("preview.pdf"), "%PDF-1.7").expect("pdf fixture");
        let state = ServiceState::local(
            GenerationIdentity {
                connection_id: "local".into(),
                connection_generation: 7,
            },
            checkout,
            dir.path().join("settings.json"),
            || true,
        )
        .expect("state");
        let router = service_router(state);

        let encoded_plus = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/file/download?path=a%2Bb.txt&connection_generation=7")
                    .body(Body::empty())
                    .expect("encoded plus request"),
            )
            .await
            .expect("encoded plus response");
        assert_eq!(encoded_plus.status(), StatusCode::OK);
        assert_eq!(
            encoded_plus.headers()["content-disposition"],
            "attachment; filename=\"a+b.txt\"; filename*=UTF-8''a%2Bb.txt"
        );
        assert_eq!(
            encoded_plus
                .into_body()
                .collect()
                .await
                .expect("encoded plus body")
                .to_bytes(),
            "literal-plus"
        );

        let raw_plus = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/file/download?path=a+b.txt&connection_generation=7")
                    .body(Body::empty())
                    .expect("raw plus request"),
            )
            .await
            .expect("raw plus response");
        assert_eq!(raw_plus.status(), StatusCode::OK);
        assert_eq!(
            raw_plus
                .into_body()
                .collect()
                .await
                .expect("raw plus body")
                .to_bytes(),
            "space"
        );

        let inline = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/file/download?path=preview.pdf&inline=true&connection_generation=7")
                    .body(Body::empty())
                    .expect("inline request"),
            )
            .await
            .expect("inline response");
        assert_eq!(inline.status(), StatusCode::OK);
        assert_eq!(inline.headers()["content-type"], "application/pdf");
        assert!(
            inline.headers()["content-disposition"]
                .to_str()
                .expect("disposition")
                .starts_with("inline;")
        );
        assert_eq!(inline.headers()["cache-control"], "private, no-store");
        assert_eq!(inline.headers()["x-content-type-options"], "nosniff");
        assert_eq!(
            inline
                .into_body()
                .collect()
                .await
                .expect("inline body")
                .to_bytes(),
            "%PDF-1.7"
        );

        let malformed = router
            .oneshot(
                Request::builder()
                    .uri("/api/file/download?path=bad%ZZ.txt&connection_generation=7")
                    .body(Body::empty())
                    .expect("malformed request"),
            )
            .await
            .expect("malformed response");
        assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn inline_boolean_contract_is_preserved() {
        let (_dir, state) = state();
        std::fs::write(
            PathBuf::from(&state.checkout.path).join("preview.pdf"),
            "%PDF-1.7",
        )
        .expect("pdf fixture");
        let router = service_router(state);

        let inline = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/file/download?path=preview.pdf&inline=true&connection_generation=7")
                    .body(Body::empty())
                    .expect("inline request"),
            )
            .await
            .expect("inline response");
        assert_eq!(inline.status(), StatusCode::OK);
        assert_eq!(inline.headers()["content-type"], "application/pdf");
        assert!(
            inline.headers()["content-disposition"]
                .to_str()
                .expect("disposition")
                .starts_with("inline;")
        );
        assert_eq!(inline.headers()["cache-control"], "private, no-store");
        assert_eq!(inline.headers()["x-content-type-options"], "nosniff");

        let attachment = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/file/download?path=preview.pdf&inline=false&connection_generation=7")
                    .body(Body::empty())
                    .expect("false inline request"),
            )
            .await
            .expect("false inline response");
        assert_eq!(attachment.status(), StatusCode::OK);
        assert_eq!(
            attachment.headers()["content-type"],
            "application/octet-stream"
        );
        assert!(
            attachment.headers()["content-disposition"]
                .to_str()
                .expect("attachment disposition")
                .starts_with("attachment;")
        );
        assert!(attachment.headers().get("cache-control").is_none());
        assert!(attachment.headers().get("x-content-type-options").is_none());

        let default_attachment = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/file/download?path=preview.pdf&connection_generation=7")
                    .body(Body::empty())
                    .expect("default inline request"),
            )
            .await
            .expect("default inline response");
        assert_eq!(default_attachment.status(), StatusCode::OK);
        assert!(
            default_attachment.headers()["content-disposition"]
                .to_str()
                .expect("default attachment disposition")
                .starts_with("attachment;")
        );

        let invalid = router
            .oneshot(
                Request::builder()
                    .uri("/api/file/download?path=preview.pdf&inline=yes&connection_generation=7")
                    .body(Body::empty())
                    .expect("invalid inline request"),
            )
            .await
            .expect("invalid inline response");
        assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn existing_file_route_contract_rejects_stale_upload_before_write() {
        let (_dir, state) = state_with_guard(|| false);
        let target = PathBuf::from(&state.checkout.path).join("stale.txt");
        let router = service_router(state);
        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/file/upload?connection_generation=7&directory=&filename=stale.txt")
                    .body(Body::from("must-not-write"))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert!(!target.exists(), "stale upload reached filesystem mutation");
    }

    #[tokio::test]
    async fn queued_http_delete_rejects_retired_generation_before_mutation() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use tokio::sync::oneshot;

        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            let current = Arc::new(AtomicBool::new(true));
            let guard = Arc::clone(&current);
            let (dir, state) = state_with_guard(move || guard.load(Ordering::Acquire));
            let target = dir.path().join("keep.txt");
            std::fs::write(&target, "keep").expect("delete target");
            let (entered_tx, entered_rx) = oneshot::channel();
            let (release_tx, release_rx) = oneshot::channel();
            let workspace = Arc::clone(&state.workspace);
            let checkout = state.checkout.clone();
            let holder = tokio::spawn(async move {
                let stream = Box::pin(futures_util::stream::once(async move {
                    // Upload reads its body only after acquiring the mutation lock.
                    entered_tx.send(()).expect("signal lock held");
                    release_rx.await.expect("release upload");
                    Ok::<_, std::io::Error>(axum::body::Bytes::new())
                }));
                let mut body = tokio_util::io::StreamReader::new(stream);
                workspace
                    .upload(
                        &checkout,
                        &json!({"directory":"","filename":"holder.txt"}),
                        &mut body,
                    )
                    .await
                    .expect("holding upload");
            });
            entered_rx.await.expect("upload holds mutation lock");

            let mut delete = Box::pin(
                service_router(state).oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/file/delete?path=keep.txt&connection_generation=7")
                        .body(Body::empty())
                        .expect("delete request"),
                ),
            );
            let (queued_tx, queued_rx) = oneshot::channel();
            let mut queued_tx = Some(queued_tx);
            let delete = tokio::spawn(std::future::poll_fn(move |cx| {
                let result = delete.as_mut().poll(cx);
                // Delete's first suspension is admission to the held mutation lock.
                if result.is_pending()
                    && let Some(queued_tx) = queued_tx.take()
                {
                    queued_tx.send(()).expect("signal queued delete");
                }
                result
            }));
            queued_rx.await.expect("delete queued behind upload");
            current.store(false, Ordering::Release);
            release_tx.send(()).expect("release mutation lock");
            holder.await.expect("upload task");
            let response = delete.await.expect("delete task").expect("delete response");
            assert_eq!(response.status(), StatusCode::CONFLICT);
            assert_eq!(
                std::fs::read_to_string(&target).ok().as_deref(),
                Some("keep"),
                "retired queued delete mutated target"
            );
        })
        .await
        .expect("queued delete regression timed out");
    }

    #[tokio::test]
    async fn malformed_rpc_identity_is_rejected() {
        let (_dir, state) = state();
        let error = dispatch_rpc(
            &state,
            &json!({"id":"bad","method":"settings.get","params":{},"connection_id":7}),
        )
        .await
        .expect_err("wrong identity type must fail");
        assert!(matches!(error, ServiceError::Invalid(_)));
    }

    #[test]
    fn query_values_are_percent_decoded() {
        assert_eq!(
            query_params("filename=quote%27%20space.txt").expect("query"),
            json!({"filename":"quote' space.txt"})
        );
        assert_eq!(
            query_params("filename=a%2Bb.txt").expect("encoded plus"),
            json!({"filename":"a+b.txt"})
        );
        assert_eq!(
            query_params("filename=a+b.txt").expect("raw plus"),
            json!({"filename":"a b.txt"})
        );
        assert_eq!(
            query_params("a%2Bb=encoded+a").expect("encoded key plus"),
            json!({"a+b":"encoded a"})
        );
        assert_eq!(
            query_params("a+b=raw").expect("raw key plus"),
            json!({"a b":"raw"})
        );
        assert!(query_params("filename=%FF").is_err());
        assert!(query_params("filename=%ZZ").is_err());
    }

    #[tokio::test]
    async fn unsupported_callbacks_are_not_reported_as_success() {
        let dir = tempfile::tempdir().expect("fixture");
        let state = ServiceState::local(
            GenerationIdentity {
                connection_id: "local".into(),
                connection_generation: 7,
            },
            Checkout {
                workspace_id: "workspace-1".into(),
                path: dir.path().to_string_lossy().into_owned(),
                repo_name: "repo".into(),
            },
            dir.path().join("settings.json"),
            || true,
        )
        .expect("state");
        let error = dispatch_rpc(
            &state,
            &json!({"id":"hooks","method":"settings.worktree_hooks.get","params":{"workspace_id":"workspace-1"}}),
        )
        .await
        .expect_err("missing hook callback must fail");
        assert!(matches!(error, ServiceError::Unsupported));
    }

    #[tokio::test]
    async fn unavailable_autosync_callback_does_not_mutate_settings() {
        let dir = tempfile::tempdir().expect("fixture");
        let settings_path = dir.path().join("settings.json");
        let state = ServiceState::local(
            GenerationIdentity {
                connection_id: "local".into(),
                connection_generation: 7,
            },
            Checkout {
                workspace_id: "workspace-1".into(),
                path: dir.path().to_string_lossy().into_owned(),
                repo_name: "repo".into(),
            },
            settings_path.clone(),
            || true,
        )
        .expect("state");
        let before = std::fs::read(&settings_path).unwrap_or_default();
        let error = dispatch_rpc(
            &state,
            &json!({"id":"sync","method":"settings.workspace_auto_sync.update_key","params":{"key":"workspace-1","enabled":true}}),
        )
        .await
        .expect_err("missing autosync callback must fail");
        assert!(matches!(error, ServiceError::Unsupported));
        assert_eq!(std::fs::read(&settings_path).unwrap_or_default(), before);
    }
}
