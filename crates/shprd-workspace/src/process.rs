use crate::{Error, Result};

use std::{
    io,
    process::Stdio,
    sync::{Mutex, OnceLock},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::oneshot,
    task::JoinHandle,
};
#[derive(Debug, Clone, Default)]
pub enum HostConfig {
    #[default]
    Local,
    /// OpenSSH alias or user@host. Host keys must already be provisioned.
    Ssh {
        destination: String,
        #[cfg(test)]
        program: std::path::PathBuf,
        #[cfg(test)]
        cleanup_observer: Option<std::sync::Arc<CleanupObservation>>,
    },
}
#[cfg(test)]
#[derive(Debug, Clone)]
pub struct CleanupObservation {
    inner: std::sync::Arc<CleanupObservationInner>,
}
#[cfg(test)]
#[derive(Debug)]
struct CleanupObservationInner {
    ready: tokio::sync::Notify,
    state: std::sync::Mutex<CleanupObservationState>,
}
#[cfg(test)]
#[derive(Debug)]
struct CleanupObservationState {
    finished: bool,
    error: Option<String>,
    reaped: bool,
    temp_path: Option<std::path::PathBuf>,
}
#[cfg(test)]
impl CleanupObservation {
    #[cfg(test)]
    pub(crate) fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            inner: std::sync::Arc::new(CleanupObservationInner {
                ready: tokio::sync::Notify::new(),
                state: std::sync::Mutex::new(CleanupObservationState {
                    finished: false,
                    error: None,
                    reaped: false,
                    temp_path: None,
                }),
            }),
        })
    }
    fn record_temp_path(&self, path: std::path::PathBuf) {
        self.inner.state.lock().unwrap().temp_path = Some(path);
    }
    pub(crate) fn temp_path(&self) -> Option<std::path::PathBuf> {
        self.inner.state.lock().unwrap().temp_path.clone()
    }
    fn finish(&self, result: io::Result<()>) -> io::Result<()> {
        let mut state = self.inner.state.lock().unwrap();
        state.finished = true;
        state.reaped = result.is_ok();
        state.error = result.err().map(|error| error.to_string());
        drop(state);
        self.inner.ready.notify_waiters();
        Ok(())
    }
    pub(crate) fn reaped(&self) -> bool {
        self.inner.state.lock().unwrap().reaped
    }

    pub(crate) async fn wait(&self) -> io::Result<()> {
        loop {
            let notified = self.inner.ready.notified();
            let result = {
                let state = self.inner.state.lock().unwrap();
                state.finished.then(|| {
                    state
                        .error
                        .as_ref()
                        .map_or(Ok(()), |error| Err(io::Error::other(error.clone())))
                })
            };
            if let Some(result) = result {
                return result;
            }
            notified.await;
        }
    }
}
impl HostConfig {
    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn test_ssh(destination: impl Into<String>, program: std::path::PathBuf) -> Self {
        Self::Ssh {
            destination: destination.into(),
            program,
            cleanup_observer: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn test_ssh_with_cleanup(
        destination: impl Into<String>,
        program: std::path::PathBuf,
        cleanup_observer: std::sync::Arc<CleanupObservation>,
    ) -> Self {
        Self::Ssh {
            destination: destination.into(),
            program,
            cleanup_observer: Some(cleanup_observer),
        }
    }

    #[cfg(test)]
    fn ssh_program(&self) -> std::path::PathBuf {
        match self {
            Self::Ssh { program, .. } => program.clone(),
            Self::Local => std::path::PathBuf::from("ssh"),
        }
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if let Self::Ssh { destination, .. } = self {
            let parts: Vec<_> = destination.split('@').collect();
            let valid_part = |part: &str, max: usize, user: bool| {
                !part.is_empty()
                    && part.len() <= max
                    && part.bytes().enumerate().all(|(i, c)| {
                        c.is_ascii_alphanumeric()
                            || (c == b'_' && (user || i > 0))
                            || (i > 0 && matches!(c, b'.' | b'-'))
                    })
            };
            let valid = match parts.as_slice() {
                [host] => valid_part(host, 253, false),
                [user, host] => valid_part(user, 64, true) && valid_part(host, 253, false),
                _ => false,
            };
            if !valid || destination.len() > 320 {
                return Err(Error::Invalid(
                    "ssh_destination must be an OpenSSH alias or user@host".into(),
                ));
            }
        }
        Ok(())
    }
    pub(crate) fn command(&self, argv: &[&str]) -> Result<Command> {
        self.validate()?;
        let Some(program) = argv.first() else {
            return Err(Error::Invalid("empty command".into()));
        };
        if argv.iter().any(|arg| arg.contains('\0')) {
            return Err(Error::Invalid("NUL in process argument".into()));
        }
        let mut command = match self {
            Self::Local => {
                let mut command = Command::new(program);
                command.args(&argv[1..]);
                command
            }
            Self::Ssh { destination, .. } => {
                #[cfg(test)]
                let program = self.ssh_program();
                #[cfg(not(test))]
                let program = std::path::PathBuf::from("ssh");
                let mut command = Command::new(program);
                for option in [
                    "BatchMode=yes",
                    "StrictHostKeyChecking=yes",
                    "PermitLocalCommand=no",
                    "RequestTTY=no",
                    "ControlMaster=no",
                    "ControlPath=none",
                    "ControlPersist=no",
                    "ConnectTimeout=8",
                    "ConnectionAttempts=1",
                ] {
                    command.args(["-o", option]);
                }
                command
                    .arg("--")
                    .arg(destination)
                    .arg(argv.iter().map(|v| quote(v)).collect::<Vec<_>>().join(" "));
                command
            }
        };
        #[cfg(unix)]
        command.process_group(0);
        command
            .kill_on_drop(true)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        Ok(command)
    }
}
pub(crate) fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
pub(crate) struct Output {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: String,
    pub truncated: bool,
}
async fn terminate(
    child: &mut tokio::process::Child,
    process_group: Option<u32>,
) -> std::io::Result<()> {
    let mut cleanup_error = None;
    #[cfg(unix)]
    if let Some(process_group) = process_group {
        let group = format!("-{process_group}");
        match Command::new("kill")
            .args(["-KILL", "--", &group])
            .stderr(Stdio::null())
            .status()
            .await
        {
            Ok(status) if status.success() => {}
            Ok(status) => {
                cleanup_error = Some(io::Error::other(format!(
                    "process-group kill exited with {status}"
                )));
            }
            Err(error) => cleanup_error = Some(error),
        }
    }
    match child.kill().await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            cleanup_error.get_or_insert(error);
        }
    }
    match child.wait().await {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            cleanup_error.get_or_insert(error);
        }
    };
    match cleanup_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}
#[cfg(test)]
#[derive(Clone, Default)]
struct CleanupHook {
    observer: Option<std::sync::Arc<CleanupObservation>>,
}
#[cfg(not(test))]
#[derive(Clone, Default)]
struct CleanupHook;
impl CleanupHook {
    #[cfg(test)]
    fn from_host(host: &HostConfig) -> Self {
        Self {
            observer: match host {
                HostConfig::Local => None,
                HostConfig::Ssh {
                    cleanup_observer, ..
                } => cleanup_observer.clone(),
            },
        }
    }
    #[cfg(not(test))]
    fn from_host(_host: &HostConfig) -> Self {
        Self
    }
    fn record_temp_path(&self, path: std::path::PathBuf) {
        #[cfg(test)]
        if let Some(observer) = &self.observer {
            observer.record_temp_path(path);
        }
        #[cfg(not(test))]
        let _ = path;
    }
    fn finish(&self, result: io::Result<()>) -> io::Result<()> {
        #[cfg(test)]
        if let Some(observer) = &self.observer {
            let observed = result
                .as_ref()
                .map(|_| ())
                .map_err(|error| io::Error::other(error.to_string()));
            observer.finish(observed)?;
        }
        result
    }
}
enum OutputSink {
    Memory(usize),
    Temp {
        file: tempfile::NamedTempFile,
        writer: tokio::fs::File,
    },
}
enum OwnedOutput {
    Memory(Output),
    Temp(tempfile::NamedTempFile, u64),
}
struct ProcessCall<T> {
    input: Option<ChildStdin>,
    cancel: Option<oneshot::Sender<()>>,
    result: Option<oneshot::Receiver<OwnerResponse<T>>>,
}
struct OwnerResponse<T> {
    result: Result<T>,
    cleanup: io::Result<()>,
}
async fn cancel_call<T>(call: ProcessCall<T>, error: Error) -> Error {
    match call.cancel_and_return(error).await {
        Ok(_) => Error::Process("cancelled process returned unexpected output".into()),
        Err(error) => error,
    }
}
impl<T> ProcessCall<T> {
    async fn send(&mut self, bytes: &[u8]) -> Result<()> {
        self.input
            .as_mut()
            .ok_or_else(|| Error::Process("process input already closed".into()))?
            .write_all(bytes)
            .await
            .map_err(Error::Io)
    }
    async fn finish_input(&mut self) -> Result<()> {
        let mut input = self
            .input
            .take()
            .ok_or_else(|| Error::Process("process input already closed".into()))?;
        input.shutdown().await.map_err(Error::Io)
    }
    fn request_cancel(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(());
        }
    }
    async fn receive_response(&mut self) -> Result<OwnerResponse<T>> {
        self.result
            .take()
            .ok_or_else(|| Error::Process("owned process result already consumed".into()))?
            .await
            .map_err(|_| Error::Process("owned process ended without a result".into()))
    }
    async fn wait(&mut self) -> Result<T> {
        let response = self.receive_response().await?;
        match response.cleanup {
            Ok(()) => response.result,
            Err(cleanup) => {
                drop(response.result);
                Err(Error::Process(format!("process cleanup failed: {cleanup}")))
            }
        }
    }
    async fn cancel_and_return(mut self, operation: Error) -> Result<T> {
        self.request_cancel();
        let response = self.receive_response().await?;
        drop(response.result);
        match response.cleanup {
            Ok(()) => Err(operation),
            Err(cleanup) => Err(Error::Process(format!(
                "{operation}; process cleanup failed: {cleanup}"
            ))),
        }
    }
}
impl<T> Drop for ProcessCall<T> {
    fn drop(&mut self) {
        self.request_cancel();
    }
}
static PROCESS_OWNERS: OnceLock<Mutex<Vec<JoinHandle<()>>>> = OnceLock::new();
fn track_owner(task: JoinHandle<()>) {
    let owners = PROCESS_OWNERS.get_or_init(|| Mutex::new(Vec::new()));
    let mut owners = owners
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    owners.retain(|task| !task.is_finished());
    owners.push(task);
}
async fn execute(
    child: &mut Child,
    stdout: ChildStdout,
    stderr: tokio::process::ChildStderr,
    sink: OutputSink,
) -> Result<OwnedOutput> {
    let stderr_read = async { bounded(stderr, 8192).await.map_err(Error::Io) };
    match sink {
        OutputSink::Memory(limit) => {
            let wait = async { child.wait().await.map_err(Error::Io) };
            let stdout_read = async { bounded(stdout, limit).await.map_err(Error::Io) };
            let ((stdout, truncated), (stderr, _), status) =
                tokio::try_join!(stdout_read, stderr_read, wait)?;
            Ok(OwnedOutput::Memory(Output {
                code: status.code().unwrap_or(-1),
                stdout,
                stderr: String::from_utf8_lossy(&stderr).into_owned(),
                truncated,
            }))
        }
        OutputSink::Temp { file, mut writer } => {
            let wait = async { child.wait().await.map_err(Error::Io) };
            let mut stdout = stdout;
            let copy = async {
                tokio::io::copy(&mut stdout, &mut writer)
                    .await
                    .map_err(Error::Io)
            };
            let (size, (stderr, _), status) = tokio::try_join!(copy, stderr_read, wait)?;
            if !status.success() {
                return Err(Error::Process(
                    String::from_utf8_lossy(&stderr).into_owned(),
                ));
            }
            writer.sync_all().await.map_err(Error::Io)?;
            Ok(OwnedOutput::Temp(file, size))
        }
    }
}
async fn own_process(
    child: Child,
    process_group: Option<u32>,
    seconds: u64,
    sink: OutputSink,
    mut cancel: oneshot::Receiver<()>,
    hook: CleanupHook,
    result: oneshot::Sender<OwnerResponse<OwnedOutput>>,
) {
    let mut child = child;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let value = match (stdout, stderr) {
        (Some(stdout), Some(stderr)) => {
            let outcome = {
                let operation = execute(&mut child, stdout, stderr, sink);
                tokio::pin!(operation);
                tokio::time::timeout(Duration::from_secs(seconds), async {
                    tokio::select! {
                        value = &mut operation => Ok(value),
                        cancelled = &mut cancel => {
                            if cancelled.is_ok() { Err(()) } else { Ok(operation.await) }
                        }
                    }
                })
                .await
            };
            let (result_value, cleanup) = match outcome {
                Ok(Ok(Ok(value))) => {
                    let cleanup = hook.finish(Ok(()));
                    (Ok(value), cleanup)
                }
                Ok(Ok(Err(error))) => {
                    let cleanup = terminate(&mut child, process_group).await;
                    let cleanup = hook.finish(
                        cleanup
                            .as_ref()
                            .map(|_| ())
                            .map_err(|error| io::Error::other(error.to_string())),
                    );
                    (Err(error), cleanup)
                }
                Ok(Err(())) => {
                    let cleanup = terminate(&mut child, process_group).await;
                    let cleanup = hook.finish(
                        cleanup
                            .as_ref()
                            .map(|_| ())
                            .map_err(|error| io::Error::other(error.to_string())),
                    );
                    (
                        Err(Error::Process("caller cancelled owned process".into())),
                        cleanup,
                    )
                }
                Err(_) => {
                    let cleanup = terminate(&mut child, process_group).await;
                    let cleanup = hook.finish(
                        cleanup
                            .as_ref()
                            .map(|_| ())
                            .map_err(|error| io::Error::other(error.to_string())),
                    );
                    (Err(Error::Timeout), cleanup)
                }
            };
            (result_value, cleanup)
        }
        _ => {
            let cleanup = terminate(&mut child, process_group).await;
            let cleanup = hook.finish(cleanup);
            (
                Err(Error::Process("owned process streams unavailable".into())),
                cleanup,
            )
        }
    };
    let _ = result.send(OwnerResponse {
        result: value.0,
        cleanup: value.1,
    });
}
fn spawn_process(
    host: &HostConfig,
    argv: &[&str],
    seconds: u64,
    sink: OutputSink,
    hook: CleanupHook,
) -> Result<ProcessCall<OwnedOutput>> {
    let mut command = host.command(argv)?;
    command.stdin(Stdio::piped());
    let mut child = command.spawn()?;
    let process_group = child.id();
    let input = child
        .stdin
        .take()
        .ok_or_else(|| Error::Process("missing stdin".into()))?;
    let (cancel, cancel_rx) = oneshot::channel();
    let (result, result_rx) = oneshot::channel();
    track_owner(tokio::spawn(own_process(
        child,
        process_group,
        seconds,
        sink,
        cancel_rx,
        hook,
        result,
    )));
    Ok(ProcessCall {
        input: Some(input),
        cancel: Some(cancel),
        result: Some(result_rx),
    })
}
impl Output {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }
    pub fn checked(self) -> Result<Self> {
        if self.code == 0 {
            Ok(self)
        } else {
            Err(self.error())
        }
    }
    pub fn error(&self) -> Error {
        let text = if self.stderr.trim().is_empty() {
            self.text()
        } else {
            self.stderr.clone()
        };
        Error::Process(if text.trim().is_empty() {
            format!("process exited {}", self.code)
        } else {
            text.trim().chars().take(2000).collect()
        })
    }
}
async fn bounded<R: AsyncRead + Unpin>(
    mut reader: R,
    limit: usize,
) -> std::io::Result<(Vec<u8>, bool)> {
    let mut result = Vec::new();
    let mut truncated = false;
    let mut buffer = vec![0; 16384];
    loop {
        let count = reader.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        let keep = count.min(limit.saturating_sub(result.len()));
        result.extend_from_slice(&buffer[..keep]);
        truncated |= keep != count;
    }
    Ok((result, truncated))
}
pub(crate) async fn run(
    host: &HostConfig,
    argv: &[&str],
    input: Option<&[u8]>,
    seconds: u64,
    limit: usize,
) -> Result<Output> {
    let mut call = spawn_process(
        host,
        argv,
        seconds,
        OutputSink::Memory(limit),
        CleanupHook::from_host(host),
    )?;
    if let Some(input) = input
        && let Err(error) = call.send(input).await
    {
        return Err(cancel_call(call, error).await);
    }
    if let Err(error) = call.finish_input().await {
        return Err(cancel_call(call, error).await);
    }
    match call.wait().await? {
        OwnedOutput::Memory(output) => Ok(output),
        OwnedOutput::Temp(_, _) => {
            Err(Error::Process("unexpected temporary process output".into()))
        }
    }
}
pub(crate) async fn spool(
    host: &HostConfig,
    argv: &[&str],
    seconds: u64,
) -> Result<(tempfile::NamedTempFile, u64)> {
    let file = tempfile::NamedTempFile::new()?;
    let path = file.path().to_path_buf();
    let hook = CleanupHook::from_host(host);
    hook.record_temp_path(path);
    let mut call = spawn_process(
        host,
        argv,
        seconds,
        OutputSink::Temp {
            writer: tokio::fs::File::from_std(file.reopen()?),
            file,
        },
        hook,
    )?;
    if let Err(error) = call.finish_input().await {
        return Err(cancel_call(call, error).await);
    }
    match call.wait().await? {
        OwnedOutput::Temp(file, size) => Ok((file, size)),
        OwnedOutput::Memory(_) => Err(Error::Process("unexpected memory process output".into())),
    }
}
pub(crate) async fn stream_input<R: AsyncRead + Unpin, F: Fn() -> bool + Sync>(
    host: &HostConfig,
    argv: &[&str],
    body: &mut R,
    current: &F,
) -> Result<Output> {
    if !current() {
        return Err(Error::Stale("connection generation changed".into()));
    }
    let mut call = spawn_process(
        host,
        argv,
        120,
        OutputSink::Memory(16384),
        CleanupHook::from_host(host),
    )?;
    let feed = async {
        let mut buffer = [0u8; 64 * 1024];
        loop {
            if !current() {
                return Err(Error::Stale("connection generation changed".into()));
            }
            let count = body.read(&mut buffer).await.map_err(Error::Io)?;
            if count == 0 {
                if !current() {
                    return Err(Error::Stale("connection generation changed".into()));
                }
                break;
            }
            if !current() {
                return Err(Error::Stale("connection generation changed".into()));
            }
            call.send(&buffer[..count]).await?;
            if !current() {
                return Err(Error::Stale("connection generation changed".into()));
            }
        }
        if !current() {
            return Err(Error::Stale("connection generation changed".into()));
        }
        call.finish_input().await
    };
    match tokio::time::timeout(Duration::from_secs(120), feed).await {
        Ok(Ok(())) => match call.wait().await? {
            OwnedOutput::Memory(output) => Ok(output),
            OwnedOutput::Temp(_, _) => {
                Err(Error::Process("unexpected temporary process output".into()))
            }
        },
        Ok(Err(error)) => Err(cancel_call(call, error).await),
        Err(_) => Err(cancel_call(call, Error::Timeout).await),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::{
        fs::OpenOptions,
        process::Command as StdCommand,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        task::{Context, Poll, Waker},
    };
    use tokio::io::AsyncBufReadExt;

    struct ReadGate {
        entered: tokio::sync::Notify,
        state: std::sync::Mutex<ReadGateState>,
    }

    struct ReadGateState {
        released: bool,
        sent: bool,
        waker: Option<Waker>,
    }

    impl ReadGate {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                entered: tokio::sync::Notify::new(),
                state: std::sync::Mutex::new(ReadGateState {
                    released: false,
                    sent: false,
                    waker: None,
                }),
            })
        }

        fn release(&self) {
            let waker = {
                let mut state = self.state.lock().unwrap();
                state.released = true;
                state.waker.take()
            };
            if let Some(waker) = waker {
                waker.wake();
            }
        }
    }

    struct GatedReader {
        gate: Arc<ReadGate>,
        body: &'static [u8],
    }

    impl AsyncRead for GatedReader {
        fn poll_read(
            self: std::pin::Pin<&mut Self>,
            cx: &mut Context<'_>,
            buffer: &mut tokio::io::ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            let mut state = self.gate.state.lock().unwrap();
            if !state.released {
                state.waker = Some(cx.waker().clone());
                drop(state);
                self.gate.entered.notify_waiters();
                return Poll::Pending;
            }
            if !state.sent {
                buffer.put_slice(self.body);
                state.sent = true;
            }
            Poll::Ready(Ok(()))
        }
    }

    #[cfg(unix)]
    fn cancellation_fixture() -> (
        tempfile::TempDir,
        HostConfig,
        tokio::fs::File,
        String,
        Arc<CleanupObservation>,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let ssh = dir.path().join("ssh");
        std::fs::write(
            &ssh,
            "#!/bin/sh\ncommand=\"\"\nfor arg in \"$@\"; do command=\"$arg\"; done\nexec /bin/bash -c \"$command\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o755)).unwrap();
        let fifo = dir.path().join("ready.fifo");
        let status = StdCommand::new("mkfifo").arg(&fifo).status().unwrap();
        assert!(status.success());
        let reader = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&fifo)
            .unwrap();
        let command = format!(
            "tail -f /dev/null & descendant=$!; printf \"%s %s\\n\" \"$$\" \"$descendant\" > {} ; wait",
            quote(fifo.to_str().unwrap())
        );
        let observer = CleanupObservation::new();
        let host = HostConfig::test_ssh_with_cleanup("test-host", ssh, Arc::clone(&observer));
        (
            dir,
            host,
            tokio::fs::File::from_std(reader),
            command,
            observer,
        )
    }

    #[cfg(unix)]
    async fn cancellation_pids<F>(reader: tokio::fs::File, task: &mut F) -> (u32, u32)
    where
        F: std::future::Future + Unpin,
    {
        let mut reader = tokio::io::BufReader::new(reader);
        let mut line = String::new();
        tokio::select! {
            result = reader.read_line(&mut line) => {
                result.unwrap();
            }
            _result = task => panic!("cancellation operation ended before owned descendant-ready event"),
        }
        let mut pids = line
            .split_whitespace()
            .map(|pid| pid.parse::<u32>().unwrap());
        (pids.next().unwrap(), pids.next().unwrap())
    }

    #[cfg(unix)]
    fn process_alive(pid: u32) -> bool {
        StdCommand::new("kill")
            .args(["-0", "--", &pid.to_string()])
            .status()
            .unwrap()
            .success()
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn caller_cancellation_run_cleans_descendant_before_ack() {
        let (_dir, host, reader, command, observer) = cancellation_fixture();
        let argv = ["bash", "-c", command.as_str(), "bash"];
        let mut task = Box::pin(run(&host, &argv, None, 120, 1024));
        let pids = cancellation_pids(reader, &mut task).await;
        drop(task);
        tokio::time::timeout(Duration::from_secs(2), observer.wait())
            .await
            .unwrap()
            .unwrap();
        assert!(observer.reaped());
        assert!(!process_alive(pids.0));
        assert!(!process_alive(pids.1));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn caller_cancellation_spool_cleans_descendant_before_ack() {
        let (_dir, host, reader, command, observer) = cancellation_fixture();
        let argv = ["bash", "-c", command.as_str(), "bash"];
        let mut task = Box::pin(spool(&host, &argv, 120));
        let pids = cancellation_pids(reader, &mut task).await;
        drop(task);
        tokio::time::timeout(Duration::from_secs(2), observer.wait())
            .await
            .unwrap()
            .unwrap();
        assert!(observer.reaped());
        assert!(!process_alive(pids.0));
        assert!(!process_alive(pids.1));
        assert!(!observer.temp_path().unwrap().exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn caller_cancellation_stream_input_cleans_descendant_before_ack() {
        let (_dir, host, reader, command, observer) = cancellation_fixture();
        let argv = ["bash", "-c", command.as_str(), "bash"];
        let mut body = tokio::io::empty();
        let mut task = Box::pin(stream_input(&host, &argv, &mut body, &|| true));
        let pids = cancellation_pids(reader, &mut task).await;
        drop(task);
        tokio::time::timeout(Duration::from_secs(2), observer.wait())
            .await
            .unwrap()
            .unwrap();
        assert!(observer.reaped());
        assert!(!process_alive(pids.0));
        assert!(!process_alive(pids.1));
    }

    #[test]
    fn ssh_argv_rejects_options_and_quotes_every_remote_argument() {
        for destination in [
            "-oProxyCommand=bad",
            "user@bad;host",
            "user@@host",
            "host/path",
            "",
            " user@host",
        ] {
            assert!(
                HostConfig::Ssh {
                    destination: destination.into(),
                    #[cfg(test)]
                    program: "ssh".into(),
                    cleanup_observer: None,
                }
                .command(&["git"])
                .is_err()
            );
        }
        let host = HostConfig::Ssh {
            destination: "user@host".into(),
            #[cfg(test)]
            program: "ssh".into(),
            cleanup_observer: None,
        };
        let command = host
            .command(&["printf", "%s", "quote' ; $(false)\nvalue"])
            .unwrap();
        let args: Vec<_> = command
            .as_std()
            .get_args()
            .map(|v| v.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&"StrictHostKeyChecking=yes".into()));
        assert!(args.contains(&"PermitLocalCommand=no".into()));
        assert_eq!(args[args.len() - 2], "user@host");
        assert_eq!(
            args.last().unwrap(),
            "'printf' '%s' 'quote'\\'' ; $(false)\nvalue'"
        );
    }
    #[tokio::test]
    async fn process_delivers_eof_and_drains_bounded_output() {
        let output = run(&HostConfig::Local, &["cat"], Some(b"content"), 2, 4)
            .await
            .unwrap()
            .checked()
            .unwrap();
        assert_eq!(output.stdout, b"cont");
        assert!(output.truncated);
        let output = stream_input(&HostConfig::Local, &["cat"], &mut &b"streamed"[..], &|| {
            true
        })
        .await
        .unwrap()
        .checked()
        .unwrap();
        assert_eq!(output.stdout, b"streamed");
        let value = "quotes' ; $(false)\nsecond";
        let shell = format!("printf '%s' {}", quote(value));
        assert_eq!(
            run(&HostConfig::Local, &["sh", "-c", &shell], None, 2, 1024)
                .await
                .unwrap()
                .checked()
                .unwrap()
                .text(),
            value
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn timed_out_ssh_process_reaps_descendants() {
        let dir = tempfile::tempdir().unwrap();
        let ssh = dir.path().join("ssh");
        std::fs::write(
            &ssh,
            "#!/bin/sh\ncommand=''\nfor arg in \"$@\"; do command=\"$arg\"; done\nexec /bin/bash -c \"$command\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o755)).unwrap();
        let pid_file = dir.path().join("descendant.pid");
        let pid_arg = pid_file.to_str().unwrap();
        let host = HostConfig::test_ssh("test-host", ssh);
        let argv = [
            "bash",
            "-c",
            r#"tail -f /dev/null & child=$!; printf '%s' "$child" > "$1"; wait"#,
            "bash",
            pid_arg,
        ];
        let result = run(&host, &argv, None, 1, 1024).await;
        assert!(
            matches!(result, Err(Error::Timeout)),
            "expected timeout, got {:?}",
            result.err()
        );
        let pid = std::fs::read_to_string(pid_file).unwrap();
        let status = StdCommand::new("kill")
            .args(["-0", pid.trim()])
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(!status.success(), "descendant process survived timeout");
    }

    #[tokio::test]
    async fn stream_input_rejects_stale_bytes_after_read_release() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("stale-bytes");
        let current = Arc::new(AtomicBool::new(true));
        let gate_current = Arc::clone(&current);
        let gate = ReadGate::new();
        let entered = gate.entered.notified();
        let reader = GatedReader {
            gate: Arc::clone(&gate),
            body: b"stale-bytes",
        };
        let mut reader = reader;
        let target_arg = target.to_str().unwrap();
        let argv = ["sh", "-c", "tee \"$1\" >/dev/null", "sh", target_arg];
        let current_check = move || gate_current.load(Ordering::Acquire);
        let mut task = Box::pin(stream_input(
            &HostConfig::Local,
            &argv,
            &mut reader,
            &current_check,
        ));
        tokio::pin!(entered);
        tokio::select! {
            () = &mut entered => {}
            _result = &mut task => panic!("stream ended before gated read"),
        }
        current.store(false, Ordering::Release);
        gate.release();
        let error = match task.await {
            Ok(_) => panic!("retired stream must reject bytes"),
            Err(error) => error,
        };
        assert!(matches!(error, Error::Stale(_)));
        assert_eq!(std::fs::read(&target).unwrap_or_default(), b"");
    }

    #[tokio::test]
    async fn stream_input_rejects_stale_eof_before_shutdown() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("stale-eof");
        let current = Arc::new(AtomicBool::new(true));
        let gate_current = Arc::clone(&current);
        let gate = ReadGate::new();
        let entered = gate.entered.notified();
        let reader = GatedReader {
            gate: Arc::clone(&gate),
            body: b"",
        };
        let mut reader = reader;
        let target_arg = target.to_str().unwrap();
        let argv = [
            "sh",
            "-c",
            "while IFS= read -r line; do :; done; printf '%s' eof > \"$1\"",
            "sh",
            target_arg,
        ];
        let current_check = move || gate_current.load(Ordering::Acquire);
        let mut task = Box::pin(stream_input(
            &HostConfig::Local,
            &argv,
            &mut reader,
            &current_check,
        ));
        tokio::pin!(entered);
        tokio::select! {
            () = &mut entered => {}
            _result = &mut task => panic!("stream ended before gated EOF read"),
        }
        current.store(false, Ordering::Release);
        gate.release();
        let error = match task.await {
            Ok(_) => panic!("retired EOF must reject shutdown"),
            Err(error) => error,
        };
        assert!(matches!(error, Error::Stale(_)));
        assert!(!target.exists());
    }

    #[tokio::test]
    async fn stream_input_current_generation_publishes_eof_control() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("current-eof");
        let target_arg = target.to_str().unwrap();
        let argv = [
            "sh",
            "-c",
            "while IFS= read -r line; do :; done; printf '%s' eof > \"$1\"",
            "sh",
            target_arg,
        ];
        let mut body = tokio::io::empty();
        let output = stream_input(&HostConfig::Local, &argv, &mut body, &|| true)
            .await
            .unwrap();
        assert_eq!(output.code, 0);
        assert_eq!(std::fs::read(target).unwrap(), b"eof");
    }

    #[tokio::test]
    async fn stream_input_current_generation_publishes_control() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("current");
        let gate = ReadGate::new();
        gate.release();
        let reader = GatedReader {
            gate,
            body: b"current-bytes",
        };
        let mut reader = reader;
        let target_arg = target.to_str().unwrap();
        let argv = ["sh", "-c", "tee \"$1\" >/dev/null", "sh", target_arg];
        let output = stream_input(&HostConfig::Local, &argv, &mut reader, &|| true)
            .await
            .unwrap();
        assert_eq!(output.code, 0);
        assert_eq!(std::fs::read(target).unwrap(), b"current-bytes");
    }
}
