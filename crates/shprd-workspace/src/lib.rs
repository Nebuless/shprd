//! File and Git services for a checkout resolved by the connection host.
#![forbid(unsafe_code)]
mod files;
mod git;
mod process;
mod remote;
pub use files::Download;
pub use process::HostConfig;
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};
use tokio::{io::AsyncRead, sync::Mutex};
pub const PREVIEW_MAX_BYTES: usize = 512 * 1024;
pub const PREVIEW_IMAGE_MAX_BYTES: usize = 5 * 1024 * 1024;
pub const GIT_DIFF_MAX_BYTES: usize = 512 * 1024;
pub const LIST_LIMIT: usize = 1000;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Stale(String),
    #[error("{0}")]
    Process(String),
    #[error("workspace operation timed out")]
    Timeout,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
pub type Result<T> = std::result::Result<T, Error>;
/// Stable workspace identity; resolve pane fallbacks before calling this crate.
#[derive(Debug, Clone)]
pub struct Checkout {
    pub workspace_id: String,
    pub path: String,
    pub repo_name: String,
}
#[derive(Debug, Clone)]
struct Range {
    root: String,
    baseline: String,
    current: String,
}
#[derive(Debug, Default)]
struct Baselines {
    active: HashMap<String, (String, String)>,
    completed: HashMap<String, Range>,
    snapshots: VecDeque<(String, String, Range)>,
    serial: u64,
}
/// One instance per connection generation. Dropping it discards baseline ownership.
#[derive(Debug, Default)]
pub struct WorkspaceService {
    host: HostConfig,
    baselines: Mutex<Baselines>,
    mutation: Mutex<()>,
}
impl WorkspaceService {
    pub fn new(host: HostConfig) -> Result<Self> {
        host.validate()?;
        Ok(Self {
            host,
            ..Self::default()
        })
    }
    /// Dispatch JSON RPC after the caller has acquired a connection-generation lease.
    pub async fn dispatch(
        &self,
        checkout: &Checkout,
        method: &str,
        params: &Value,
    ) -> Result<Value> {
        validate_request(checkout, params)?;
        match method {
            "file.list" | "file.resolve" | "file.read" | "file.delete" => {
                let mut value = files::dispatch(&self.host, checkout, method, params).await?;
                value["workspace_id"] = json!(checkout.workspace_id);
                if method == "file.list" || method == "file.read" {
                    value["repo_name"] = json!(checkout.repo_name);
                    value["checkout_path"] = json!(checkout.path);
                }
                Ok(value)
            }
            "git.diff_summary" | "git.diff_file" | "git.file_action" | "git.repo_action"
            | "git.pull" | "git.status" => {
                let _guard = self.mutation.lock().await;
                git::dispatch(self, checkout, method, params).await
            }
            _ => Err(Error::Invalid(format!(
                "unsupported workspace method: {method}"
            ))),
        }
    }
    /// Spools process output on disk; regular local files are opened directly.
    pub async fn download(&self, checkout: &Checkout, params: &Value) -> Result<Download> {
        validate_request(checkout, params)?;
        files::download(&self.host, checkout, params).await
    }
    /// Copies in bounded chunks and publishes only after the full body succeeds.
    pub async fn upload<R: AsyncRead + Unpin>(
        &self,
        checkout: &Checkout,
        params: &Value,
        body: &mut R,
    ) -> Result<Value> {
        validate_request(checkout, params)?;
        let _guard = self.mutation.lock().await;
        files::upload(&self.host, checkout, params, body).await
    }
    /// Call at activity start, before the engine can edit files.
    pub async fn capture_workspace(&self, checkout: &Checkout) -> Result<String> {
        let _guard = self.mutation.lock().await;
        let root = git::root(&self.host, &checkout.path).await?;
        let tree = git::snapshot(&self.host, &root).await?;
        self.baselines
            .lock()
            .await
            .active
            .insert(checkout.workspace_id.clone(), (root, tree.clone()));
        Ok(tree)
    }
    /// Failed completion preserves the previous completed range.
    pub async fn complete_workspace(&self, checkout: &Checkout) -> Result<bool> {
        let _guard = self.mutation.lock().await;
        let mut store = self.baselines.lock().await;
        let Some((root, baseline)) = store.active.get(&checkout.workspace_id).cloned() else {
            return Ok(false);
        };
        if git::root(&self.host, &checkout.path).await? != root {
            return Ok(false);
        }
        let current = git::snapshot(&self.host, &root).await?;
        store.completed.insert(
            checkout.workspace_id.clone(),
            Range {
                root,
                baseline,
                current,
            },
        );
        store.active.remove(&checkout.workspace_id);
        store
            .snapshots
            .retain(|(_, workspace, _)| workspace != &checkout.workspace_id);
        Ok(true)
    }
    pub async fn invalidate_workspace(&self, workspace_id: &str) {
        let mut store = self.baselines.lock().await;
        store.active.remove(workspace_id);
        store.completed.remove(workspace_id);
        store
            .snapshots
            .retain(|(_, workspace, _)| workspace != workspace_id);
    }
}
fn validate_request(checkout: &Checkout, params: &Value) -> Result<()> {
    if checkout.workspace_id.is_empty() || checkout.path.is_empty() || checkout.path.contains('\0')
    {
        return Err(Error::Invalid(
            "workspace has no directory path or workspace_id".into(),
        ));
    }
    if !params.is_object() {
        return Err(Error::Invalid("params must be an object".into()));
    }
    if let Some(id) = params.get("workspace_id")
        && id.as_str() != Some(&checkout.workspace_id)
    {
        return Err(Error::Invalid(
            "workspace_id does not match resolved checkout".into(),
        ));
    }
    Ok(())
}
fn string<'a>(params: &'a Value, key: &str) -> &'a str {
    params.get(key).and_then(Value::as_str).unwrap_or("")
}
fn path(value: &str, preview: bool) -> Result<String> {
    let raw = if preview { value.trim() } else { value };
    let normalized = raw.replace('\\', "/");
    let parts: Vec<_> = normalized.split('/').filter(|p| !p.is_empty()).collect();
    if parts.iter().any(|p| *p == ".." || p.contains('\0')) {
        return Err(Error::Invalid(format!(
            "invalid file {} path",
            if preview { "preview" } else { "explorer" }
        )));
    }
    let joined = parts.join("/");
    Ok(if preview && normalized.starts_with('/') {
        format!("/{joined}")
    } else {
        joined
    })
}
fn required_path(params: &Value, preview: bool) -> Result<String> {
    let path = path(string(params, "path"), preview)?;
    if path.is_empty() || path.split('/').all(|part| part.is_empty() || part == ".") {
        return Err(Error::Invalid("operation requires path".into()));
    }
    Ok(path)
}
