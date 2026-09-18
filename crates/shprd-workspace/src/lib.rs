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
pub const IMAGE_UPLOAD_MAX_BYTES: usize = 25 * 1024 * 1024;

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
        self.dispatch_if_current(checkout, method, params, || true)
            .await
    }
    pub async fn dispatch_if_current<F>(
        &self,
        checkout: &Checkout,
        method: &str,
        params: &Value,
        current: F,
    ) -> Result<Value>
    where
        F: Fn() -> bool + Send + Sync,
    {
        validate_request(checkout, params)?;
        if !current() {
            return Err(Error::Stale("connection generation changed".into()));
        }
        match method {
            "file.list" | "file.resolve" | "file.read" => {
                let mut value =
                    files::dispatch(&self.host, checkout, method, params, &current).await?;
                value["workspace_id"] = json!(checkout.workspace_id);
                if method == "file.list" || method == "file.read" {
                    value["repo_name"] = json!(checkout.repo_name);
                    value["checkout_path"] = json!(checkout.path);
                }
                Ok(value)
            }
            "file.delete" => {
                let _guard = self.mutation.lock().await;
                if !current() {
                    return Err(Error::Stale("connection generation changed".into()));
                }
                let mut value =
                    files::dispatch(&self.host, checkout, method, params, &current).await?;
                value["workspace_id"] = json!(checkout.workspace_id);
                Ok(value)
            }
            "git.diff_summary" | "git.diff_file" | "git.file_action" | "git.repo_action"
            | "git.pull" | "git.status" => {
                let _guard = self.mutation.lock().await;
                if !current() {
                    return Err(Error::Stale("connection generation changed".into()));
                }
                git::dispatch(self, checkout, method, params, &current).await
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
        self.upload_if_current(checkout, params, body, || true)
            .await
    }
    pub async fn upload_if_current<R, F>(
        &self,
        checkout: &Checkout,
        params: &Value,
        body: &mut R,
        current: F,
    ) -> Result<Value>
    where
        R: AsyncRead + Unpin,
        F: Fn() -> bool + Send + Sync,
    {
        validate_request(checkout, params)?;
        if !current() {
            return Err(Error::Stale("connection generation changed".into()));
        }
        let _guard = self.mutation.lock().await;
        if !current() {
            return Err(Error::Stale("connection generation changed".into()));
        }
        files::upload(&self.host, checkout, params, body, &current).await
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

/// Stores an image in the connection host's temporary directory without terminal input.
pub async fn upload_terminal_image<R, F>(
    host: &HostConfig,
    extension: &str,
    body: &mut R,
    current: F,
) -> Result<String>
where
    R: AsyncRead + Unpin,
    F: Fn() -> bool + Send + Sync,
{
    files::upload_terminal_image(host, extension, body, &current).await
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

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod safety_tests {
    use super::*;
    use std::{
        future::Future,
        process::Command,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        task::{Context, Poll, Waker},
    };

    fn checkout(root: &std::path::Path) -> Checkout {
        Checkout {
            workspace_id: "safety-workspace".into(),
            path: root.to_string_lossy().into_owned(),
            repo_name: "safety-repo".into(),
        }
    }

    fn git(root: &std::path::Path, args: &[&str]) -> String {
        let mut command = Command::new("git");
        command.arg("-C").arg(root).args(args);
        for (variable, _) in std::env::vars().filter(|(key, _)| key.starts_with("GIT_")) {
            command.env_remove(variable);
        }
        let output = command
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("git process");
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("git output")
    }

    fn repository() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("repository fixture");
        git(dir.path(), &["init", "-b", "main"]);
        git(dir.path(), &["config", "user.email", "test@example.com"]);
        git(dir.path(), &["config", "user.name", "SHPRD Test"]);
        std::fs::write(dir.path().join("tracked.txt"), "base\n").expect("tracked fixture");
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-m", "initial"]);
        dir
    }

    #[tokio::test]
    async fn stale_generation_cannot_admit_queued_git_mutation() {
        let dir = repository();
        std::fs::write(dir.path().join("new.txt"), "new\n").expect("new fixture");
        let checkout = checkout(dir.path());
        let service = WorkspaceService::default();
        let holder = service.mutation.lock().await;
        let current = Arc::new(AtomicBool::new(true));
        let contender_current = Arc::clone(&current);
        let params = json!({"action":"stage_all"});
        let mut contender = Box::pin(service.dispatch_if_current(
            &checkout,
            "git.repo_action",
            &params,
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
        let error = contender.await.expect_err("stale queued Git action");
        assert!(matches!(error, Error::Stale(_)));
        assert!(!git(dir.path(), &["status", "--porcelain"]).contains("A  new.txt"));
    }

    #[tokio::test]
    async fn current_generation_admits_git_mutation() {
        let dir = repository();
        std::fs::write(dir.path().join("new.txt"), "new\n").expect("new fixture");
        let checkout = checkout(dir.path());
        let service = WorkspaceService::default();
        service
            .dispatch_if_current(
                &checkout,
                "git.repo_action",
                &json!({"action":"stage_all"}),
                || true,
            )
            .await
            .expect("current Git action");
        assert!(git(dir.path(), &["status", "--porcelain"]).contains("A  new.txt"));
    }

    #[tokio::test]
    async fn stale_generation_cannot_admit_queued_delete() {
        let dir = tempfile::tempdir().expect("delete fixture");
        let target = dir.path().join("delete-me.txt");
        std::fs::write(&target, "keep").expect("delete target");
        let checkout = checkout(dir.path());
        let service = WorkspaceService::default();
        let holder = service.mutation.lock().await;
        let current = Arc::new(AtomicBool::new(true));
        let contender_current = Arc::clone(&current);
        let params = json!({"path":"delete-me.txt"});
        let mut contender = Box::pin(service.dispatch_if_current(
            &checkout,
            "file.delete",
            &params,
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
        let error = contender.await.expect_err("stale queued delete");
        assert!(matches!(error, Error::Stale(_)));
        assert!(target.exists());
    }

    #[tokio::test]
    async fn current_generation_deletes_file() {
        let dir = tempfile::tempdir().expect("delete fixture");
        let target = dir.path().join("delete-me.txt");
        std::fs::write(&target, "remove").expect("delete target");
        let checkout = checkout(dir.path());
        WorkspaceService::default()
            .dispatch_if_current(
                &checkout,
                "file.delete",
                &json!({"path":"delete-me.txt"}),
                || true,
            )
            .await
            .expect("current delete");
        assert!(!target.exists());
    }

    #[cfg(unix)]
    fn fake_ssh() -> (tempfile::TempDir, HostConfig) {
        use std::{fs::File, io::Write, os::unix::fs::PermissionsExt};
        let dir = tempfile::tempdir().expect("fake SSH fixture");
        let path = dir.path().join("ssh");
        let mut file = File::create(&path).expect("fake SSH program");
        writeln!(
            file,
            "#!/bin/sh\ncommand=''\nfor arg in \"$@\"; do command=\"$arg\"; done\n/bin/bash -c \"$command\"\nstatus=$?\n: > \"$0.stage\"\nexit \"$status\""
        )
        .expect("fake SSH source");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("fake SSH permissions");
        let host = HostConfig::test_ssh("test-host", path);
        (dir, host)
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn ssh_terminal_image_upload_streams_to_temp_path() {
        let (fake_dir, host) = fake_ssh();
        let mut body = std::io::Cursor::new(b"terminal image".to_vec());

        let path = upload_terminal_image(&host, "PNG", &mut body, || true)
            .await
            .expect("remote image upload");

        assert!(path.starts_with("/tmp/herdr-img-"));
        assert!(path.ends_with(".png"));
        assert_eq!(
            std::fs::read(&path).expect("remote image content"),
            b"terminal image"
        );
        std::fs::remove_file(&path).expect("remote image cleanup");
        drop(fake_dir);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stale_generation_cannot_publish_queued_remote_upload() {
        let (fake_dir, host) = fake_ssh();
        let fake_program = fake_dir.path().join("ssh");
        let checkout_dir = tempfile::tempdir().expect("remote checkout fixture");
        let checkout = checkout(checkout_dir.path());
        let service = WorkspaceService::new(host).expect("workspace service");
        let holder = service.mutation.lock().await;
        let current = Arc::new(AtomicBool::new(true));
        let contender_current = Arc::clone(&current);
        let params = json!({"directory":"","filename":"remote.txt"});
        let mut body = std::io::Cursor::new(b"must-not-publish".to_vec());
        let mut contender = Box::pin(service.upload_if_current(
            &checkout,
            &params,
            &mut body,
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
        let error = contender.await.expect_err("stale queued remote upload");
        assert!(matches!(error, Error::Stale(_)));
        assert!(!checkout_dir.path().join("remote.txt").exists());
        drop(fake_dir);
        assert!(!fake_program.exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn current_generation_publishes_remote_upload() {
        let (fake_dir, host) = fake_ssh();
        let fake_program = fake_dir.path().join("ssh");
        let checkout_dir = tempfile::tempdir().expect("remote checkout fixture");
        let checkout = checkout(checkout_dir.path());
        let service = WorkspaceService::new(host).expect("workspace service");
        let mut body = std::io::Cursor::new(b"published".to_vec());
        service
            .upload_if_current(
                &checkout,
                &json!({"directory":"","filename":"remote.txt"}),
                &mut body,
                || true,
            )
            .await
            .expect("current remote upload");
        assert_eq!(
            std::fs::read(checkout_dir.path().join("remote.txt")).expect("remote target"),
            b"published"
        );
        drop(fake_dir);
        assert!(!fake_program.exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stale_generation_cannot_commit_remote_upload_after_staging() {
        let (fake_dir, host) = fake_ssh();
        let fake_program = fake_dir.path().join("ssh");
        let checkout_dir = tempfile::tempdir().expect("remote checkout fixture");
        let target = checkout_dir.path().join("remote.txt");
        std::fs::write(&target, "original").expect("remote target");
        let checkout = checkout(checkout_dir.path());
        let retired = Arc::new(AtomicBool::new(false));
        let current_retired = Arc::clone(&retired);
        let stage_marker = fake_program.with_extension("stage");
        let stage_marker_check = stage_marker.clone();
        let mut body = std::io::Cursor::new(b"committed-too-early".to_vec());
        let error = remote::upload(&host, &checkout, "", "remote.txt", &mut body, &move || {
            if stage_marker_check.exists() {
                current_retired.store(true, Ordering::Release);
            }
            !current_retired.load(Ordering::Acquire)
        })
        .await
        .expect_err("retirement before final commit admission");
        assert!(matches!(error, Error::Stale(_)));
        assert!(stage_marker.exists());
        assert_eq!(std::fs::read(&target).unwrap(), b"original");
        assert_eq!(
            std::fs::read_dir(checkout_dir.path())
                .unwrap()
                .filter_map(std::result::Result::ok)
                .filter(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".shprd-upload.")
                })
                .count(),
            0
        );
        drop(fake_dir);
        assert!(!fake_program.exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn admitted_remote_commit_can_return_stale_without_rollback_claim() {
        let (fake_dir, host) = fake_ssh();
        let fake_program = fake_dir.path().join("ssh");
        let checkout_dir = tempfile::tempdir().expect("remote checkout fixture");
        let checkout = checkout(checkout_dir.path());
        let retired = Arc::new(AtomicBool::new(false));
        let observed_target = checkout_dir.path().join("remote.txt");
        let guard_target = observed_target.clone();
        let current_retired = Arc::clone(&retired);
        let mut body = std::io::Cursor::new(b"admitted-remote-commit".to_vec());
        let result = remote::upload(&host, &checkout, "", "remote.txt", &mut body, &move || {
            if guard_target.exists() {
                current_retired.store(true, Ordering::Release);
            }
            !current_retired.load(Ordering::Acquire)
        })
        .await;
        assert!(matches!(result, Err(Error::Stale(_))));
        assert_eq!(
            std::fs::read(&observed_target).unwrap(),
            b"admitted-remote-commit"
        );
        assert_eq!(
            std::fs::read_dir(checkout_dir.path())
                .unwrap()
                .filter_map(std::result::Result::ok)
                .filter(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".shprd-upload.")
                })
                .count(),
            0
        );
        drop(fake_dir);
        assert!(!fake_program.exists());
    }
}
