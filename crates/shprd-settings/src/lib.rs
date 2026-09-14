//! Persistent GUI settings with connection-owned repository namespaces.
#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{
    collections::BTreeMap,
    fs::Metadata,
    path::{Path, PathBuf},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio::{
    fs,
    sync::{Mutex, MutexGuard},
};

pub const DEFAULT_WORKSPACE_AUTO_SYNC_INTERVAL_MINUTES: u64 = 10;
const LEGACY_DEFAULT_CONNECTION_ID: &str = "legacy-default";
const MAX_KEY_COMPONENT: usize = 4096;
static SETTINGS_MUTATION_QUEUE: OnceLock<Mutex<()>> = OnceLock::new();
static TEMPORARY_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("invalid settings path: {0}")]
    InvalidPath(String),
    #[error("settings path cannot be a symbolic link")]
    Symlink,
    #[error("settings key belongs to another connection")]
    Ownership,
    #[error("unknown workspace auto-sync config: {0}")]
    UnknownAutoSync(String),
    #[error("settings update cancelled")]
    Cancelled,
    #[error("connection generation changed")]
    Stale,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone)]
pub struct SettingsIdentity {
    pub connection_id: Option<String>,
    pub host: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceMetadata {
    pub workspace_id: String,
    pub label: Option<String>,
    pub repo_key: Option<String>,
    pub repo_root: Option<String>,
    pub checkout_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct GuiRepoSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_hooks_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom: Option<Map<String, Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GuiWorkspaceAutoSyncSettings {
    pub enabled: bool,
    pub interval_minutes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkout_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_run_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_status: Option<WorkspaceAutoSyncStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_branch: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AutoSyncResult {
    pub checkout_path: String,
    pub host: Option<String>,
    pub status: WorkspaceAutoSyncStatus,
    pub message: Option<String>,
    pub branch: Option<String>,
    pub last_run_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceAutoSyncStatus {
    Updated,
    UpToDate,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GuiSettings {
    pub version: u8,
    pub repositories: BTreeMap<String, GuiRepoSettings>,
    pub workspace_auto_sync: BTreeMap<String, GuiWorkspaceAutoSyncSettings>,
    pub custom: Map<String, Value>,
}

impl Default for GuiSettings {
    fn default() -> Self {
        Self {
            version: 1,
            repositories: BTreeMap::new(),
            workspace_auto_sync: BTreeMap::new(),
            custom: Map::new(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RepoSettingsPatch {
    pub worktree_hooks_enabled: Option<bool>,
    pub custom: Option<Map<String, Value>>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WorkspaceAutoSyncView {
    pub workspace_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_label: Option<String>,
    pub checkout_path: String,
    pub key: String,
    pub enabled: bool,
    pub interval_minutes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_run_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_status: Option<WorkspaceAutoSyncStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_branch: Option<String>,
    pub running: bool,
}

/// Hook configuration discovered by the coordinator from paseo.json.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WorktreeHookConfig {
    pub setup: Option<String>,
    pub opened: Option<String>,
    pub teardown: Option<String>,
    pub removed: Option<String>,
}

/// Coordinator-owned paseo discovery result supplied to hook response assembly.
#[derive(Debug, Clone)]
pub struct WorktreeHookDiscovery {
    pub path: String,
    pub config: WorktreeHookConfig,
}

/// Hook preference plus coordinator-supplied discovery result.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WorktreeHooksView {
    pub workspace_id: String,
    pub key: Option<String>,
    pub enabled: bool,
    pub repo_name: Option<String>,
    pub repo_root: Option<String>,
    pub checkout_path: Option<String>,
    pub source_checkout_path: Option<String>,
    pub paseo_path: Option<String>,
    pub hooks: Value,
    pub error: Option<String>,
}

#[derive(Debug)]
struct Inner {
    path: PathBuf,
    identity: SettingsIdentity,
}

/// Settings service. Clone shares serialized mutation queue and path ownership.
#[derive(Debug, Clone)]
pub struct SettingsService {
    inner: Arc<Inner>,
}

impl SettingsService {
    pub fn new(path: impl AsRef<Path>, identity: SettingsIdentity) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        validate_settings_path(&path)?;
        Ok(Self {
            inner: Arc::new(Inner { path, identity }),
        })
    }
    pub fn path(&self) -> &Path {
        &self.inner.path
    }
    pub fn identity(&self) -> &SettingsIdentity {
        &self.inner.identity
    }
    pub async fn read(&self) -> Result<GuiSettings> {
        self.load().await
    }
    pub async fn read_json(&self) -> Result<Value> {
        Ok(serde_json::to_value(self.load().await?)?)
    }

    pub async fn update_repo(&self, key: &str, patch: RepoSettingsPatch) -> Result<GuiSettings> {
        self.update_repo_if_current(key, patch, || true).await
    }
    pub async fn update_repo_if_current<F>(
        &self,
        key: &str,
        patch: RepoSettingsPatch,
        current: F,
    ) -> Result<GuiSettings>
    where
        F: Fn() -> bool + Send + Sync,
    {
        self.assert_owned(key)?;
        self.mutate_if_current(
            |mut settings| {
                let existing = settings.repositories.entry(key.to_owned()).or_default();
                if let Some(enabled) = patch.worktree_hooks_enabled {
                    existing.worktree_hooks_enabled = Some(enabled);
                }
                if let Some(custom) = patch.custom {
                    let target = existing.custom.get_or_insert_with(Map::new);
                    target.extend(custom);
                }
                Ok(settings)
            },
            &current,
        )
        .await
    }
    pub async fn repo_worktree_hooks_enabled(&self, key: Option<&str>) -> Result<bool> {
        let Some(key) = key else { return Ok(true) };
        self.assert_owned(key)?;
        Ok(self
            .load()
            .await?
            .repositories
            .get(key)
            .and_then(|v| v.worktree_hooks_enabled)
            .unwrap_or(true))
    }
    /// Build hook settings response from coordinator-owned paseo discovery.
    pub async fn worktree_hooks_view(
        &self,
        workspace: &WorkspaceMetadata,
        discovery: Option<WorktreeHookDiscovery>,
        error: Option<String>,
    ) -> Result<WorktreeHooksView> {
        let key = workspace_repo_settings_key(workspace, self.identity())?;
        let enabled = self.repo_worktree_hooks_enabled(key.as_deref()).await?;
        Ok(WorktreeHooksView {
            workspace_id: workspace.workspace_id.clone(),
            key,
            enabled,
            repo_name: workspace.label.clone(),
            repo_root: workspace.repo_root.clone(),
            checkout_path: workspace.checkout_path.clone(),
            source_checkout_path: workspace
                .repo_root
                .clone()
                .or_else(|| workspace.checkout_path.clone()),
            paseo_path: discovery.as_ref().map(|value| value.path.clone()),
            hooks: discovery
                .map(|value| serde_json::to_value(value.config))
                .transpose()?
                .unwrap_or_else(|| json!({})),
            error,
        })
    }
    pub async fn list_workspace_auto_sync<F>(&self, running: &F) -> Result<Vec<Value>>
    where
        F: Fn(&str) -> bool + Sync,
    {
        let settings = self.load().await?;
        Ok(settings
            .workspace_auto_sync
            .into_iter()
            .filter(|(key, _)| self.owns(key))
            .map(|(key, entry)| {
                let mut value = serde_json::to_value(entry).unwrap_or_else(|_| json!({}));
                value["key"] = json!(&key);
                value["running"] = json!(running(&key));
                value
            })
            .collect())
    }
    pub async fn update_auto_sync_key(
        &self,
        key: &str,
        enabled: bool,
    ) -> Result<GuiWorkspaceAutoSyncSettings> {
        self.update_auto_sync_key_if_current(key, enabled, || true)
            .await
    }
    pub async fn update_auto_sync_key_if_current<F>(
        &self,
        key: &str,
        enabled: bool,
        current: F,
    ) -> Result<GuiWorkspaceAutoSyncSettings>
    where
        F: Fn() -> bool + Send + Sync,
    {
        self.assert_owned(key)?;
        let settings = self
            .mutate_if_current(
                |mut settings| {
                    let entry = settings
                        .workspace_auto_sync
                        .get_mut(key)
                        .ok_or_else(|| Error::UnknownAutoSync(key.to_owned()))?;
                    entry.enabled = enabled;
                    Ok(settings)
                },
                &current,
            )
            .await?;
        settings
            .workspace_auto_sync
            .get(key)
            .cloned()
            .ok_or_else(|| Error::UnknownAutoSync(key.to_owned()))
    }
    /// Record one coordinator-owned auto-sync attempt without creating unknown entries.
    pub async fn record_auto_sync_result(&self, key: &str, result: AutoSyncResult) -> Result<bool> {
        self.assert_owned(key)?;
        let settings = self
            .mutate(|mut settings| {
                let Some(entry) = settings.workspace_auto_sync.get_mut(key) else {
                    return Ok(settings);
                };
                entry.checkout_path = Some(result.checkout_path);
                entry.host = result.host;
                entry.last_status = Some(result.status);
                entry.last_message = result.message;
                entry.last_branch = result.branch;
                entry.last_run_at = Some(result.last_run_at);
                Ok(settings)
            })
            .await?;
        Ok(settings.workspace_auto_sync.contains_key(key))
    }
    pub async fn workspace_auto_sync(
        &self,
        workspace: &WorkspaceMetadata,
        running: bool,
    ) -> Result<WorkspaceAutoSyncView> {
        let checkout = workspace
            .checkout_path
            .clone()
            .or_else(|| workspace.repo_root.clone())
            .unwrap_or_default();
        let key = workspace_auto_sync_settings_key(&checkout, self.identity())?;
        let entry = self.load().await?.workspace_auto_sync.get(&key).cloned();
        Ok(auto_sync_view(workspace, checkout, key, entry, running))
    }
    pub async fn update_workspace_auto_sync(
        &self,
        workspace: &WorkspaceMetadata,
        enabled: bool,
        running: bool,
    ) -> Result<WorkspaceAutoSyncView> {
        self.update_workspace_auto_sync_if_current(workspace, enabled, running, || true)
            .await
    }
    pub async fn update_workspace_auto_sync_if_current<F>(
        &self,
        workspace: &WorkspaceMetadata,
        enabled: bool,
        running: bool,
        current: F,
    ) -> Result<WorkspaceAutoSyncView>
    where
        F: Fn() -> bool + Send + Sync,
    {
        let checkout = workspace
            .checkout_path
            .clone()
            .or_else(|| workspace.repo_root.clone())
            .unwrap_or_default();
        let key = workspace_auto_sync_settings_key(&checkout, self.identity())?;
        let settings = self
            .mutate_if_current(
                |mut settings| {
                    let old = settings.workspace_auto_sync.get(&key).cloned();
                    settings.workspace_auto_sync.insert(
                        key.clone(),
                        GuiWorkspaceAutoSyncSettings {
                            enabled,
                            interval_minutes: old
                                .as_ref()
                                .map_or(DEFAULT_WORKSPACE_AUTO_SYNC_INTERVAL_MINUTES, |v| {
                                    v.interval_minutes
                                }),
                            checkout_path: Some(checkout.clone()),
                            host: self.inner.identity.host.clone(),
                            last_run_at: old.as_ref().and_then(|v| v.last_run_at.clone()),
                            last_status: old.as_ref().and_then(|v| v.last_status.clone()),
                            last_message: old.as_ref().and_then(|v| v.last_message.clone()),
                            last_branch: old.and_then(|v| v.last_branch),
                        },
                    );
                    Ok(settings)
                },
                &current,
            )
            .await?;
        let entry = settings.workspace_auto_sync.get(&key).cloned();
        Ok(auto_sync_view(workspace, checkout, key, entry, running))
    }
    async fn load(&self) -> Result<GuiSettings> {
        validate_settings_path(&self.inner.path)?;
        match fs::read_to_string(&self.inner.path).await {
            Ok(text) => Ok(normalize(
                serde_json::from_str(&text).unwrap_or(Value::Null),
            )),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(GuiSettings::default())
            }
            Err(error) => Err(error.into()),
        }
    }
    async fn mutate<F>(&self, update: F) -> Result<GuiSettings>
    where
        F: FnOnce(GuiSettings) -> Result<GuiSettings>,
    {
        self.mutate_if_current(update, &|| true).await
    }
    async fn mutate_if_current<F, C>(&self, update: F, current: &C) -> Result<GuiSettings>
    where
        F: FnOnce(GuiSettings) -> Result<GuiSettings>,
        C: Fn() -> bool + Sync,
    {
        if !current() {
            return Err(Error::Stale);
        }
        let queue = SETTINGS_MUTATION_QUEUE.get_or_init(|| Mutex::new(()));
        let guard = queue.lock().await;
        if !current() {
            return Err(Error::Stale);
        }
        let current_settings = self.load().await?;
        if !current() {
            return Err(Error::Stale);
        }
        let next = update(current_settings)?;
        if !current() {
            return Err(Error::Stale);
        }
        self.persist(&next, guard).await
    }
    async fn persist(
        &self,
        settings: &GuiSettings,
        guard: MutexGuard<'static, ()>,
    ) -> Result<GuiSettings> {
        let path = self.inner.path.clone();
        let settings = normalize(serde_json::to_value(settings)?);
        let text = format!("{}\n", serde_json::to_string_pretty(&settings)?);
        // Once queued, persistence owns the mutation lock through rename or cleanup.
        tokio::task::spawn_blocking(move || {
            use std::io::Write;
            #[cfg(unix)]
            use std::os::unix::fs::OpenOptionsExt;
            let _guard = guard;
            validate_settings_path(&path)?;
            let parent = path
                .parent()
                .ok_or_else(|| Error::InvalidPath("settings path has no parent".into()))?;
            std::fs::create_dir_all(parent)?;
            let sequence = TEMPORARY_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;
            let temporary = parent.join(format!(
                ".{}.{}.{}.tmp",
                path.file_name()
                    .and_then(|v| v.to_str())
                    .unwrap_or("settings"),
                std::process::id(),
                sequence
            ));
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            options.mode(0o600);
            let mut file = options.open(&temporary)?;
            let result = (|| {
                file.write_all(text.as_bytes())?;
                file.sync_all()?;
                drop(file);
                std::fs::rename(&temporary, &path)
            })();
            if let Err(error) = result {
                std::fs::remove_file(&temporary)?;
                return Err(Error::Io(error));
            }
            Ok(settings)
        })
        .await
        .map_err(|_| Error::Cancelled)?
    }
    fn owns(&self, key: &str) -> bool {
        key.starts_with(&settings_prefix(&self.inner.identity))
    }
    fn assert_owned(&self, key: &str) -> Result<()> {
        if self.owns(key) {
            Ok(())
        } else {
            Err(Error::Ownership)
        }
    }
}

fn auto_sync_view(
    workspace: &WorkspaceMetadata,
    checkout: String,
    key: String,
    entry: Option<GuiWorkspaceAutoSyncSettings>,
    running: bool,
) -> WorkspaceAutoSyncView {
    let entry = entry.unwrap_or(GuiWorkspaceAutoSyncSettings {
        enabled: false,
        interval_minutes: DEFAULT_WORKSPACE_AUTO_SYNC_INTERVAL_MINUTES,
        checkout_path: None,
        host: None,
        last_run_at: None,
        last_status: None,
        last_message: None,
        last_branch: None,
    });
    WorkspaceAutoSyncView {
        workspace_id: workspace.workspace_id.clone(),
        workspace_label: workspace.label.clone(),
        checkout_path: checkout,
        key,
        enabled: entry.enabled,
        interval_minutes: entry.interval_minutes,
        last_run_at: entry.last_run_at,
        last_status: entry.last_status,
        last_message: entry.last_message,
        last_branch: entry.last_branch,
        running,
    }
}

pub fn connection_settings_prefix(connection_id: Option<&str>) -> String {
    match connection_id.filter(|v| !v.is_empty() && *v != LEGACY_DEFAULT_CONNECTION_ID) {
        Some(value) => format!("connection:{}:", encode_component(value)),
        None => String::new(),
    }
}
pub fn settings_prefix(identity: &SettingsIdentity) -> String {
    format!(
        "{}{}:",
        connection_settings_prefix(identity.connection_id.as_deref()),
        identity
            .host
            .as_deref()
            .filter(|host| !host.is_empty())
            .map_or("local".to_owned(), |host| format!("ssh:{host}"))
    )
}
pub fn repo_settings_key(raw: &str, identity: &SettingsIdentity) -> Result<String> {
    component(raw)?;
    Ok(format!("{}{raw}", settings_prefix(identity)))
}
pub fn workspace_repo_settings_key(
    workspace: &WorkspaceMetadata,
    identity: &SettingsIdentity,
) -> Result<Option<String>> {
    let raw = workspace
        .repo_key
        .as_deref()
        .or(workspace.repo_root.as_deref())
        .or(workspace.checkout_path.as_deref())
        .unwrap_or("");
    if raw.is_empty() {
        return Ok(None);
    }
    Ok(Some(repo_settings_key(raw, identity)?))
}
pub fn workspace_auto_sync_settings_key(
    checkout: &str,
    identity: &SettingsIdentity,
) -> Result<String> {
    let checkout = checkout.trim();
    if checkout.is_empty() {
        return Err(Error::InvalidPath("workspace has no checkout path".into()));
    }
    repo_settings_key(checkout, identity)
}
fn component(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > MAX_KEY_COMPONENT || value.contains('\0') {
        Err(Error::InvalidPath("invalid settings key component".into()))
    } else {
        Ok(())
    }
}
fn encode_component(value: &str) -> String {
    use std::fmt::Write;
    value.bytes().fold(String::new(), |mut out, byte| {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push('%');
            let _ = write!(out, "{byte:02X}");
        }
        out
    })
}
fn validate_settings_path(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() || path.file_name().is_none() {
        return Err(Error::InvalidPath(path.display().to_string()));
    }
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(Error::Symlink),
        Ok(metadata) if !metadata.is_file() => Err(Error::InvalidPath(
            "settings path is not a regular file".into(),
        )),
        _ => Ok(()),
    }
}
#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod safety_tests {
    use super::*;
    use std::{
        future::Future,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        task::{Context, Poll, Waker},
    };

    fn identity() -> SettingsIdentity {
        SettingsIdentity {
            connection_id: Some("safety".into()),
            host: None,
        }
    }

    #[tokio::test]
    async fn stale_generation_cannot_persist_queued_mutation() {
        let directory = tempfile::tempdir().expect("settings fixture");
        let path = directory.path().join("settings.json");
        let service = SettingsService::new(&path, identity()).expect("settings service");
        std::fs::write(&path, "{\"custom\":{\"keep\":true}}\n").expect("seed settings");
        let before = std::fs::read(&path).expect("seed bytes");
        let queue = SETTINGS_MUTATION_QUEUE.get_or_init(|| Mutex::new(()));
        let holder = queue.lock().await;
        let current = Arc::new(AtomicBool::new(true));
        let contender_current = Arc::clone(&current);
        let mut contender = Box::pin(service.update_repo_if_current(
            "connection:safety:local:repo",
            RepoSettingsPatch::default(),
            move || contender_current.load(Ordering::Acquire),
        ));
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        assert!(matches!(
            contender.as_mut().poll(&mut context),
            Poll::Pending
        ));
        current.store(false, Ordering::Release);
        drop(holder);
        let error = contender.await.expect_err("stale queued settings mutation");
        assert!(matches!(error, Error::Stale));
        assert_eq!(std::fs::read(&path).expect("settings bytes"), before);
    }

    #[tokio::test]
    async fn current_generation_persists_settings_mutation() {
        let directory = tempfile::tempdir().expect("settings fixture");
        let path = directory.path().join("settings.json");
        let service = SettingsService::new(&path, identity()).expect("settings service");
        service
            .update_repo_if_current(
                "connection:safety:local:repo",
                RepoSettingsPatch {
                    worktree_hooks_enabled: Some(false),
                    custom: None,
                },
                || true,
            )
            .await
            .expect("current settings mutation");
        let settings = service.read().await.expect("settings read");
        assert_eq!(
            settings.repositories["connection:safety:local:repo"].worktree_hooks_enabled,
            Some(false)
        );
    }
}

fn normalize(raw: Value) -> GuiSettings {
    let Some(object) = raw.as_object() else {
        return GuiSettings::default();
    };
    let repositories = object
        .get("repositories")
        .and_then(Value::as_object)
        .map(|values| {
            values
                .iter()
                .filter_map(|(key, value)| Some((key.clone(), normalize_repo(value)?)))
                .collect()
        })
        .unwrap_or_default();
    let workspace_auto_sync = object
        .get("workspace_auto_sync")
        .and_then(Value::as_object)
        .map(|values| {
            values
                .iter()
                .filter_map(|(key, value)| Some((key.clone(), normalize_auto_sync(value)?)))
                .collect()
        })
        .unwrap_or_default();
    let custom = object
        .get("custom")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    GuiSettings {
        version: 1,
        repositories,
        workspace_auto_sync,
        custom,
    }
}
fn normalize_repo(value: &Value) -> Option<GuiRepoSettings> {
    let object = value.as_object()?;
    let worktree_hooks_enabled = object
        .get("worktree_hooks_enabled")
        .and_then(Value::as_bool);
    let custom = object.get("custom").and_then(Value::as_object).cloned();
    Some(GuiRepoSettings {
        worktree_hooks_enabled,
        custom,
    })
}
fn normalize_auto_sync(value: &Value) -> Option<GuiWorkspaceAutoSyncSettings> {
    let object = value.as_object()?;
    let interval = object
        .get("interval_minutes")
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite() && *v >= 1.0)
        .map_or(DEFAULT_WORKSPACE_AUTO_SYNC_INTERVAL_MINUTES, |v| {
            v.round()
                .to_string()
                .parse::<u64>()
                .unwrap_or(DEFAULT_WORKSPACE_AUTO_SYNC_INTERVAL_MINUTES)
        });
    let status = object
        .get("last_status")
        .and_then(Value::as_str)
        .and_then(|v| match v {
            "updated" => Some(WorkspaceAutoSyncStatus::Updated),
            "up_to_date" => Some(WorkspaceAutoSyncStatus::UpToDate),
            "skipped" => Some(WorkspaceAutoSyncStatus::Skipped),
            "failed" => Some(WorkspaceAutoSyncStatus::Failed),
            _ => None,
        });
    Some(GuiWorkspaceAutoSyncSettings {
        enabled: object
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        interval_minutes: interval,
        checkout_path: object
            .get("checkout_path")
            .and_then(Value::as_str)
            .map(str::to_owned),
        host: object
            .get("host")
            .and_then(Value::as_str)
            .map(str::to_owned),
        last_run_at: object
            .get("last_run_at")
            .and_then(Value::as_str)
            .map(str::to_owned),
        last_status: status,
        last_message: object
            .get("last_message")
            .and_then(Value::as_str)
            .map(str::to_owned),
        last_branch: object
            .get("last_branch")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

#[allow(dead_code)]
fn _metadata_is_regular(metadata: &Metadata) -> bool {
    metadata.is_file()
}

#[cfg(test)]
mod cancellation_tests {
    use super::*;
    use std::{future::Future, task::Poll};

    #[test]
    fn dropping_persistence_keeps_transaction_owned_until_finished()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("settings.json");
        let service = SettingsService::new(
            &path,
            SettingsIdentity {
                connection_id: None,
                host: None,
            },
        )?;
        let settings = GuiSettings::default();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .max_blocking_threads(1)
            .build()?;
        let (started, received) = tokio::sync::oneshot::channel();
        let (release, resume) = std::sync::mpsc::channel();
        runtime.block_on(async {
            // Occupy the only blocking worker before starting persistence.
            let blocker = tokio::task::spawn_blocking(move || {
                let _ = started.send(());
                resume.recv_timeout(std::time::Duration::from_secs(5))
            });
            received.await?;
            let queue = SETTINGS_MUTATION_QUEUE.get_or_init(|| Mutex::new(()));
            let guard = queue.lock().await;
            let mut persistence = Box::pin(service.persist(&settings, guard));
            std::future::poll_fn(|cx| match persistence.as_mut().poll(cx) {
                Poll::Pending => Poll::Ready(Ok(())),
                Poll::Ready(_) => Poll::Ready(Err("persistence unexpectedly completed")),
            })
            .await?;
            drop(persistence);
            assert!(queue.try_lock().is_err());
            release.send(())?;
            blocker.await??;
            Ok::<(), Box<dyn std::error::Error>>(())
        })?;
        // Runtime shutdown drains already-owned blocking writes, no timing wait.
        drop(runtime);
        let actual: GuiSettings = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
        assert_eq!(actual, settings);
        assert_eq!(std::fs::read_dir(directory.path())?.count(), 1);
        Ok(())
    }
}
