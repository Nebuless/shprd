//! Concrete local/SSH connection runtime owned by the native host.
use crate::{herdr, terminal};
#[cfg(test)]
use shprd_connections::SshFailure;
use shprd_connections::{
    Error, ProbeResult, Profile, ProfileFactory, Result, Runtime, RuntimeContext, RuntimeFuture,
    SocketPaths, SshTunnel, Transport,
};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Mutex, watch};

struct NativeRuntime {
    profile: Profile,
    executable: PathBuf,
    tunnel: Arc<Mutex<Option<SshTunnel>>>,
    watcher: Mutex<Option<tokio::task::JoinHandle<Result<()>>>>,
    watcher_cancel: watch::Sender<bool>,
    ssh_pid: Arc<AtomicU32>,
    ssh_paths: Arc<Mutex<Option<SocketPaths>>>,
    #[cfg(test)]
    watcher_result: Mutex<Option<Result<Option<SshFailure>>>>,
    #[cfg(test)]
    watcher_reported: Arc<tokio::sync::Notify>,
}

fn transport_error(error: impl std::fmt::Display) -> Error {
    Error::Runtime {
        message: error.to_string(),
        retryable: true,
    }
}

async fn probe_paths(paths: &SocketPaths) -> Result<ProbeResult> {
    let info = herdr::call(
        &paths.control,
        "ping",
        &serde_json::json!({}),
        Duration::from_secs(8),
    )
    .await
    .map_err(transport_error)?;
    let protocol = info
        .get("protocol")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| Error::Invalid("invalid Herdr protocol".into()))?;
    let result = ProbeResult {
        ok: true,
        protocol,
        version: info
            .get("version")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
    };
    result.validate()?;
    terminal::Terminal::connect(&paths.render, protocol, 80, 24)
        .await
        .map_err(transport_error)?;
    Ok(result)
}

impl NativeRuntime {
    async fn open(&self) -> Result<SocketPaths> {
        match self.profile.transport() {
            Transport::Local {
                control_socket_path,
                client_socket_path,
            } => Ok(SocketPaths {
                control: control_socket_path.into(),
                render: client_socket_path.into(),
            }),
            Transport::Ssh { .. } => {
                let mut slot = self.tunnel.lock().await;
                *slot = Some(SshTunnel::spawn(&self.profile, &self.executable).await?);
                let tunnel = slot
                    .as_mut()
                    .ok_or_else(|| Error::Invalid("missing SSH tunnel".into()))?;
                tunnel.wait_ready().await?;
                self.ssh_pid
                    .store(tunnel.pid().unwrap_or_default(), Ordering::Release);
                let paths = tunnel.paths().clone();
                *self.ssh_paths.lock().await = Some(paths.clone());
                Ok(paths)
            }
        }
    }
}

impl Runtime for NativeRuntime {
    fn start<'a>(&'a self, context: &'a RuntimeContext) -> RuntimeFuture<'a, SocketPaths> {
        Box::pin(async move {
            let paths = self.open().await?;
            probe_paths(&paths).await?;
            let context = context.clone();
            let mut cancelled = self.watcher_cancel.subscribe();
            let watcher = match self.profile.transport() {
                Transport::Local {
                    control_socket_path,
                    ..
                } => {
                    let path = control_socket_path.clone();
                    tokio::spawn(async move {
                        let selectors = serde_json::json!([
                            {"type":"workspace.created"},
                            {"type":"workspace.updated"},
                            {"type":"workspace.renamed"},
                            {"type":"workspace.closed"},
                            {"type":"workspace.focused"},
                            {"type":"workspace.moved"},
                            {"type":"workspace.reordered"},
                            {"type":"tab.created"},
                            {"type":"tab.closed"},
                            {"type":"tab.renamed"},
                            {"type":"tab.focused"},
                            {"type":"pane.created"},
                            {"type":"pane.closed"},
                            {"type":"pane.focused"},
                            {"type":"pane.moved"},
                            {"type":"pane.exited"},
                            {"type":"pane.agent_detected"},
                            {"type":"layout.updated"},
                            {"type":"worktree.created"},
                            {"type":"worktree.opened"},
                            {"type":"worktree.removed"}
                        ]);
                        let mut subscription = tokio::select! {
                            _ = cancelled.changed() => return Ok(()),
                            result = herdr::Subscription::open(
                                Path::new(&path),
                                &selectors,
                                Duration::from_secs(8),
                            ) => match result {
                                Ok(subscription) => subscription,
                                Err(error) => {
                                    context.report_error(&error.to_string(), true)?;
                                    return Ok(());
                                }
                            },
                        };
                        loop {
                            tokio::select! {
                                _ = cancelled.changed() => return Ok(()),
                                result = subscription.next() => match result {
                                    Ok(_) => {},
                                    Err(error) => {
                                        context.report_error(&error.to_string(), true)?;
                                        return Ok(());
                                    }
                                },
                            }
                        }
                    })
                }
                Transport::Ssh { .. } => {
                    let tunnel = self.tunnel.clone();
                    #[cfg(test)]
                    let injected_result = self.watcher_result.lock().await.take();
                    #[cfg(test)]
                    let watcher_reported = Arc::clone(&self.watcher_reported);
                    tokio::spawn(async move {
                        let result = if let Some(result) = {
                            #[cfg(test)]
                            {
                                injected_result
                            }
                            #[cfg(not(test))]
                            {
                                None
                            }
                        } {
                            result
                        } else {
                            let mut slot = tunnel.lock().await;
                            let Some(tunnel) = slot.as_mut() else {
                                return Err(Error::Invalid("missing SSH tunnel".into()));
                            };
                            tokio::select! {
                                _ = cancelled.changed() => Ok(None),
                                result = tunnel.exited() => result.map(Some),
                            }
                        };
                        match result {
                            Ok(None) => return Ok(()),
                            Ok(Some(failure)) => {
                                context.report_error(&failure.message, failure.retryable)?;
                                #[cfg(test)]
                                watcher_reported.notify_one();
                            }
                            Err(error) => {
                                context.report_error(
                                    &error.to_string(),
                                    matches!(
                                        error,
                                        Error::Io(_)
                                            | Error::Runtime {
                                                retryable: true,
                                                ..
                                            }
                                    ),
                                )?;
                                #[cfg(test)]
                                watcher_reported.notify_one();
                            }
                        }
                        Ok(())
                    })
                }
            };
            *self.watcher.lock().await = Some(watcher);
            Ok(paths)
        })
    }
    fn stop(&self) -> RuntimeFuture<'_, ()> {
        Box::pin(async {
            self.watcher_cancel.send_replace(true);
            let watcher_result = if let Some(watcher) = self.watcher.lock().await.take() {
                watcher
                    .await
                    .map_err(|error| Error::Invalid(format!("runtime watcher failed: {error}")))?
            } else {
                Ok(())
            };
            let mut slot = self.tunnel.lock().await;
            let tunnel_result = if let Some(tunnel) = slot.as_mut() {
                let result = tunnel.stop().await;
                if result.is_ok() {
                    self.ssh_pid.store(0, Ordering::Release);
                    *self.ssh_paths.lock().await = None;
                    *slot = None;
                }
                result
            } else {
                Ok(())
            };
            watcher_result.and(tunnel_result)
        })
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use shprd_connections::{ConnectionId, Manager, Profile, State};
    use std::{os::unix::fs::PermissionsExt, sync::Arc};
    use tokio::{
        io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
        net::UnixListener,
    };

    #[tokio::test]
    async fn local_event_socket_eof_invalidates_real_runtime_generation()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let control_path = directory.path().join("control.sock");
        let render_path = directory.path().join("render.sock");
        let control = UnixListener::bind(&control_path)?;
        let render = UnixListener::bind(&render_path)?;
        let (ready_sender, ready_receiver) = tokio::sync::oneshot::channel();
        let (close_sender, close_receiver) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (control_socket, _) = control.accept().await?;
            let mut control_reader = BufReader::new(control_socket);
            let mut ping = String::new();
            control_reader.read_line(&mut ping).await?;
            control_reader
                .get_mut()
                .write_all(b"{\"id\":\"rpc\",\"result\":{\"protocol\":22}}\n")
                .await?;
            drop(control_reader);

            let (mut render_socket, _) = render.accept().await?;
            let mut length = [0; 4];
            render_socket.read_exact(&mut length).await?;
            let mut hello = vec![0; u32::from_le_bytes(length) as usize];
            render_socket.read_exact(&mut hello).await?;
            let response = bincode::encode_to_vec(
                (0_u32, 22_u32, 1_u32, Option::<String>::None),
                bincode::config::standard(),
            )
            .map_err(|error| {
                std::io::Error::other(format!("encode terminal handshake: {error}"))
            })?;
            render_socket
                .write_all(&(response.len() as u32).to_le_bytes())
                .await?;
            render_socket.write_all(&response).await?;
            drop(render_socket);

            let (control_socket, _) = control.accept().await?;
            let mut subscription = BufReader::new(control_socket);
            let mut request = String::new();
            subscription.read_line(&mut request).await?;
            subscription
                .get_mut()
                .write_all(b"{\"id\":\"sub\",\"result\":{}}\n")
                .await?;
            ready_sender
                .send(())
                .map_err(|_| std::io::Error::other("EOF fixture ready signal dropped"))?;
            close_receiver
                .await
                .map_err(|_| std::io::Error::other("EOF trigger dropped"))?;
            drop(subscription);
            Ok::<_, std::io::Error>(())
        });
        let id = ConnectionId::parse("eof")?;
        let profile = Profile::from_value(serde_json::json!({
            "id":"eof",
            "label":"EOF",
            "type":"local",
            "control_socket_path":control_path,
            "client_socket_path":render_path,
            "auto_connect":false
        }))?;
        let manager = Manager::new(ConnectionId::parse("default")?);
        let runtime_factory = factory()(&profile);
        manager.register(profile, runtime_factory)?;
        manager.connect(&id).await?;
        ready_receiver
            .await
            .map_err(|_| "EOF fixture did not acknowledge subscription")?;
        let lease = manager.lease(&id)?;
        let (armed_sender, armed_receiver) = tokio::sync::oneshot::channel();
        let (cancelled_sender, cancelled_receiver) = tokio::sync::oneshot::channel();
        let observed_lease = lease.clone();
        let cancellation_task = tokio::spawn(async move {
            armed_sender
                .send(())
                .map_err(|_| "EOF cancellation observer dropped")?;
            observed_lease.cancelled().await;
            cancelled_sender
                .send(())
                .map_err(|_| "EOF cancellation result dropped")?;
            Ok::<_, &'static str>(())
        });
        armed_receiver
            .await
            .map_err(|_| "EOF cancellation observer did not arm")?;
        close_sender
            .send(())
            .map_err(|_| "event fixture not waiting for EOF trigger")?;
        tokio::time::timeout(Duration::from_secs(2), cancelled_receiver)
            .await
            .map_err(|error| format!("lease did not cancel: {error}"))??;
        cancellation_task.await??;
        let status = manager.status(&id)?;
        assert_eq!(status.state, State::Reconnecting);
        assert!(status.generation > lease.generation());
        manager.stop_all().await?;
        server.await??;
        Ok(())
    }

    fn ssh_wrapper_fixture_script() -> &'static str {
        r#"#!/bin/sh
control=""
render=""
previous=""
for arg in "$@"; do
    if [ "$previous" = "-L" ]; then
        if [ -z "$control" ]; then control="${arg%%:*}"; else render="${arg%%:*}"; fi
    fi
    previous="$arg"
done
exec python3 - "$control" "$render" <<'PY'
import signal
import socket
import sys
import threading

control_path, render_path = sys.argv[1:]

def accept(listener):
    listener.settimeout(8)
    return listener.accept()

def recv_line(conn):
    data = b""
    while not data.endswith(b"\n"):
        data += conn.recv(4096)
    return data

def control_server():
    listener = socket.socket(socket.AF_UNIX)
    listener.bind(control_path)
    listener.listen(4)
    conn, _ = accept(listener)
    conn.close()
    conn, _ = accept(listener)
    recv_line(conn)
    conn.sendall(b'{"id":"rpc","result":{"protocol":22}}\n')
    conn.close()
    conn, _ = accept(listener)
    recv_line(conn)
    conn.sendall(b'{"id":"sub","result":{}}\n')
    conn.close()

def render_server():
    listener = socket.socket(socket.AF_UNIX)
    listener.bind(render_path)
    listener.listen(2)
    conn, _ = accept(listener)
    conn.close()
    conn, _ = accept(listener)
    length = conn.recv(4)
    if len(length) == 4:
        conn.recv(int.from_bytes(length, "little"))
        response = bytes([0, 22, 1, 0])
        conn.sendall(len(response).to_bytes(4, "little") + response)
    conn.close()

threading.Thread(target=control_server).start()
threading.Thread(target=render_server).start()
signal.pause()
PY
"#
    }

    async fn run_ssh_watcher_case(
        watcher_result: Result<Option<SshFailure>>,
    ) -> std::result::Result<State, Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let executable = directory.path().join("ssh-fixture");
        std::fs::write(&executable, ssh_wrapper_fixture_script())?;
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))?;
        let profile = Profile::from_value(serde_json::json!({
            "id":"ssh-wrapper",
            "label":"SSH wrapper",
            "type":"ssh",
            "ssh_destination":"fixture",
            "remote_control_socket_path":"/tmp/control",
            "remote_client_socket_path":"/tmp/render",
            "auto_connect":false
        }))?;
        let id = ConnectionId::parse("ssh-wrapper")?;
        let manager = Manager::new(ConnectionId::parse("default")?);
        let watcher_reported = Arc::new(tokio::sync::Notify::new());
        let runtime = Arc::new(NativeRuntime {
            profile: profile.clone(),
            executable,
            tunnel: Arc::new(Mutex::new(None)),
            watcher: Mutex::new(None),
            watcher_cancel: watch::channel(false).0,
            ssh_pid: Arc::new(AtomicU32::new(0)),
            ssh_paths: Arc::new(Mutex::new(None)),
            watcher_result: Mutex::new(Some(watcher_result)),
            watcher_reported: Arc::clone(&watcher_reported),
        });
        let runtime_handle = Arc::clone(&runtime);
        manager.register(
            profile,
            Arc::new(move |_| {
                let runtime: Arc<dyn Runtime> = runtime_handle.clone();
                Ok(runtime)
            }),
        )?;
        manager.connect(&id).await?;
        tokio::time::timeout(Duration::from_secs(2), watcher_reported.notified())
            .await
            .map_err(|error| format!("SSH watcher did not report wrapper error: {error}"))?;
        let state = manager.status(&id)?.state;
        let control_path = runtime
            .ssh_paths
            .lock()
            .await
            .as_ref()
            .ok_or("SSH wrapper paths missing")?
            .control
            .clone();
        manager.disconnect(&id).await?;
        assert!(runtime.tunnel.lock().await.is_none());
        assert!(
            !control_path
                .parent()
                .ok_or("SSH path parent missing")?
                .exists()
        );
        Ok(state)
    }

    #[tokio::test]
    async fn ssh_watcher_reports_typed_and_wrapper_failures()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            run_ssh_watcher_case(Ok(Some(SshFailure {
                message: "authentication failed".into(),
                retryable: false,
                kind: shprd_connections::SshFailureKind::Authentication,
                exit_code: 255,
            })))
            .await?,
            State::Error
        );
        assert_eq!(
            run_ssh_watcher_case(Ok(Some(SshFailure {
                message: "host key failed".into(),
                retryable: false,
                kind: shprd_connections::SshFailureKind::HostKey,
                exit_code: 255,
            })))
            .await?,
            State::Error
        );
        assert_eq!(
            run_ssh_watcher_case(Err(Error::Invalid("missing SSH tunnel".into()))).await?,
            State::Error
        );
        assert_eq!(
            run_ssh_watcher_case(Err(Error::Io(std::io::Error::other("temporary pipe")))).await?,
            State::Reconnecting
        );
        Ok(())
    }

    #[tokio::test]
    async fn ssh_child_exit_invalidates_real_runtime_generation()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let executable = directory.path().join("ssh-fixture");
        std::fs::write(
            &executable,
            r#"#!/bin/sh
control=""
render=""
previous=""
for arg in "$@"; do
    if [ "$previous" = "-L" ]; then
        if [ -z "$control" ]; then control="${arg%%:*}"; else render="${arg%%:*}"; fi
    fi
    previous="$arg"
done
exec python3 - "$control" "$render" <<'PY'
import os
import signal
import socket
import sys
import threading

SOCKET_TIMEOUT = 8
control_path, render_path = sys.argv[1:]

def accept(listener):
    listener.settimeout(SOCKET_TIMEOUT)
    return listener.accept()

def recv_line(conn):
    conn.settimeout(SOCKET_TIMEOUT)
    data = b""
    while not data.endswith(b"\n"):
        chunk = conn.recv(4096)
        if not chunk:
            raise RuntimeError("fixture peer closed before newline")
        data += chunk
    return data

def control_server():
    listener = socket.socket(socket.AF_UNIX)
    listener.bind(control_path)
    listener.listen(4)
    conn, _ = accept(listener)
    conn.close()
    conn, _ = accept(listener)
    recv_line(conn)
    conn.sendall(b'{"id":"rpc","result":{"protocol":22}}\n')
    conn.close()
    conn, _ = accept(listener)
    recv_line(conn)
    conn.sendall(b'{"id":"sub","result":{}}\n')
    recv_line(conn)
    conn.close()

def render_server():
    listener = socket.socket(socket.AF_UNIX)
    listener.bind(render_path)
    listener.listen(2)
    conn, _ = accept(listener)
    conn.close()
    conn, _ = accept(listener)
    conn.settimeout(SOCKET_TIMEOUT)
    conn.recv(4096)
    conn.sendall(b"\x04\x00\x00\x00\x00\x16\x01\x00")
    conn.close()

control_thread = threading.Thread(target=control_server)
render_thread = threading.Thread(target=render_server)
control_thread.start()
render_thread.start()
print("Permission denied (publickey)", file=sys.stderr, flush=True)
signal.pause()
PY
"#,
        )?;
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))?;
        let profile = Profile::from_value(serde_json::json!({
            "id":"ssh-exit",
            "label":"SSH exit",
            "type":"ssh",
            "ssh_destination":"fixture",
            "remote_control_socket_path":"/tmp/control",
            "remote_client_socket_path":"/tmp/render",
            "auto_connect":false
        }))?;
        let id = ConnectionId::parse("ssh-exit")?;
        let manager = Manager::new(ConnectionId::parse("default")?);
        let runtime = Arc::new(NativeRuntime {
            profile: profile.clone(),
            executable,
            tunnel: Arc::new(Mutex::new(None)),
            watcher: Mutex::new(None),
            watcher_cancel: watch::channel(false).0,
            ssh_pid: Arc::new(AtomicU32::new(0)),
            ssh_paths: Arc::new(Mutex::new(None)),
            watcher_result: Mutex::new(None),
            watcher_reported: Arc::new(tokio::sync::Notify::new()),
        });
        let runtime_handle = Arc::clone(&runtime);
        manager.register(
            profile,
            Arc::new(move |_| {
                let runtime: Arc<dyn Runtime> = runtime_handle.clone();
                Ok(runtime)
            }),
        )?;
        manager.connect(&id).await?;
        let lease = manager.lease(&id)?;
        let (armed_sender, armed_receiver) = tokio::sync::oneshot::channel();
        let (cancelled_sender, cancelled_receiver) = tokio::sync::oneshot::channel();
        let observed_lease = lease.clone();
        let cancellation_task = tokio::spawn(async move {
            armed_sender
                .send(())
                .map_err(|_| "SSH cancellation observer dropped")?;
            observed_lease.cancelled().await;
            cancelled_sender
                .send(())
                .map_err(|_| "SSH cancellation result dropped")?;
            Ok::<_, &'static str>(())
        });
        armed_receiver
            .await
            .map_err(|_| "SSH cancellation observer did not arm")?;
        let pid = runtime.ssh_pid.load(Ordering::Acquire);
        assert_ne!(pid, 0, "SSH fixture process missing");
        let control_path = runtime
            .ssh_paths
            .lock()
            .await
            .as_ref()
            .ok_or("SSH fixture paths missing before exit")?
            .control
            .clone();
        let terminated = tokio::process::Command::new("kill")
            .arg("-TERM")
            .arg(pid.to_string())
            .status()
            .await?;
        assert!(
            terminated.success(),
            "failed to signal SSH fixture: {terminated}"
        );
        tokio::time::timeout(Duration::from_secs(2), cancelled_receiver)
            .await
            .map_err(|error| format!("SSH child exit not observed: {error}"))??;
        cancellation_task.await??;
        let status = manager.status(&id)?;
        assert_eq!(status.state, State::Error);
        assert!(status.generation > lease.generation());
        assert!(
            status
                .error
                .as_ref()
                .is_some_and(|error| error.message.contains("authentication"))
        );
        tokio::task::yield_now().await;
        assert_eq!(manager.status(&id)?.state, State::Error);
        manager.stop_all().await?;
        assert!(runtime.tunnel.lock().await.is_none());
        assert!(
            !control_path
                .parent()
                .ok_or("SSH fixture control path has no parent")?
                .exists()
        );
        Ok(())
    }
}

pub fn factory() -> ProfileFactory {
    Arc::new(|profile| {
        let profile = profile.clone();
        Arc::new(move |_| {
            let (watcher_cancel, _) = watch::channel(false);
            Ok(Arc::new(NativeRuntime {
                profile: profile.clone(),
                executable: PathBuf::from("ssh"),
                tunnel: Arc::new(Mutex::new(None)),
                watcher: Mutex::new(None),
                watcher_cancel,
                ssh_pid: Arc::new(AtomicU32::new(0)),
                ssh_paths: Arc::new(Mutex::new(None)),
                #[cfg(test)]
                watcher_result: Mutex::new(None),
                #[cfg(test)]
                watcher_reported: Arc::new(tokio::sync::Notify::new()),
            }))
        })
    })
}

pub async fn probe(profile: Profile) -> Result<ProbeResult> {
    let (watcher_cancel, _) = watch::channel(false);
    let runtime = NativeRuntime {
        profile,
        executable: PathBuf::from("ssh"),
        tunnel: Arc::new(Mutex::new(None)),
        watcher: Mutex::new(None),
        watcher_cancel,
        ssh_pid: Arc::new(AtomicU32::new(0)),
        ssh_paths: Arc::new(Mutex::new(None)),
        #[cfg(test)]
        watcher_result: Mutex::new(None),
        #[cfg(test)]
        watcher_reported: Arc::new(tokio::sync::Notify::new()),
    };
    let result = async {
        let paths = runtime.open().await?;
        probe_paths(&paths).await
    }
    .await;
    let cleanup = runtime.stop().await;
    let result = result?;
    cleanup?;
    Ok(result)
}
