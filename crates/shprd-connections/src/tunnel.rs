use crate::{
    Error, Profile, Result, SocketPaths,
    error::invalid,
    home_probe::remote_home,
    paths::validate_remote_path,
    ssh::{classify_ssh_failure, ssh_profile, ssh_tunnel_argv},
};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::{Child, Command},
    task::JoinHandle,
    time::timeout,
};

pub struct SshTunnel {
    child: Child,
    directory: Option<tempfile::TempDir>,
    paths: SocketPaths,
    stderr: Option<JoinHandle<std::io::Result<String>>>,
}
pub(crate) async fn bounded_tail(
    mut reader: impl AsyncRead + Unpin,
    cap: usize,
) -> std::io::Result<String> {
    let mut retained = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = reader.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        retained.extend_from_slice(&chunk[..n]);
        if retained.len() > cap {
            retained.drain(..retained.len() - cap);
        }
    }
    Ok(String::from_utf8_lossy(&retained).into_owned())
}
impl SshTunnel {
    pub async fn spawn(profile: &Profile, executable: &Path) -> Result<Self> {
        if cfg!(windows) {
            return Err(Error::Runtime { message: "SSH connections from Windows are not supported because stream-local forwarding cannot create a Windows named pipe".into(), retryable: false });
        }
        let (host, control, render) = ssh_profile(profile)?;
        let directory = tempfile::Builder::new()
            .prefix("herdr-gui-ssh-")
            .tempdir_in("/tmp")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
        }
        let paths = SocketPaths {
            control: directory.path().join("control.sock"),
            render: directory.path().join("render.sock"),
        };
        let home = if control.is_empty() || render.is_empty() {
            Some(remote_home(executable, host).await?)
        } else {
            None
        };
        let resolve = |path: &str, file: &str| -> Result<String> {
            let resolved = if path.is_empty() {
                format!(
                    "{}/.config/herdr/{file}",
                    home.as_deref()
                        .ok_or_else(|| invalid("missing remote home"))?
                )
            } else {
                path.to_owned()
            };
            validate_remote_path(&resolved)?;
            Ok(resolved)
        };
        let remote_control = resolve(control, "herdr.sock")?;
        let remote_render = resolve(render, "herdr-client.sock")?;
        if remote_control == remote_render {
            return Err(invalid(
                "remote control and render socket paths must differ",
            ));
        }
        let local_control = paths
            .control
            .to_str()
            .ok_or_else(|| invalid("invalid local socket path"))?;
        let local_render = paths
            .render
            .to_str()
            .ok_or_else(|| invalid("invalid local socket path"))?;
        let args = ssh_tunnel_argv(
            host,
            &[
                (local_control, &remote_control),
                (local_render, &remote_render),
            ],
        )?;
        let mut child = Command::new(executable)
            .args(args)
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| invalid("missing SSH stderr"))?;
        let stderr = tokio::spawn(bounded_tail(stderr, 16 * 1024));
        Ok(Self {
            child,
            directory: Some(directory),
            paths,
            stderr: Some(stderr),
        })
    }
    pub const fn paths(&self) -> &SocketPaths {
        &self.paths
    }
    pub fn pid(&self) -> Option<u32> {
        self.child.id()
    }
    pub async fn wait_ready(&mut self) -> Result<()> {
        #[cfg(unix)]
        {
            let control = self.paths.control.clone();
            let render = self.paths.render.clone();
            let ready = async {
                let mut tick = tokio::time::interval(Duration::from_millis(100));
                loop {
                    tick.tick().await;
                    let controls = tokio::net::UnixStream::connect(&control).await;
                    let renders = tokio::net::UnixStream::connect(&render).await;
                    if controls.is_ok() && renders.is_ok() {
                        return;
                    }
                }
            };
            let outcome = tokio::select! {
                status = self.child.wait() => Some(status?),
                result = timeout(Duration::from_secs(8), ready) => { if result.is_err() { self.stop().await?; return Err(Error::Runtime { message: "ssh tunnel did not create local sockets".into(), retryable: true }); } None }
            };
            if let Some(status) = outcome {
                return Err(self.failure(status.code().unwrap_or(-1)).await?.into());
            }
            Ok(())
        }
        #[cfg(not(unix))]
        {
            Err(invalid("SSH stream-local forwarding is unsupported"))
        }
    }
    async fn failure(&mut self, code: i32) -> Result<crate::SshFailure> {
        let stderr = match self.stderr.take() {
            Some(task) => task
                .await
                .map_err(|e| invalid(format!("SSH stderr task failed: {e}")))??,
            None => String::new(),
        };
        Ok(classify_ssh_failure(code, &stderr))
    }
    /// Host races this future against shutdown and reports failure through RuntimeContext.
    pub async fn exited(&mut self) -> Result<crate::SshFailure> {
        let status = self.child.wait().await?;
        self.failure(status.code().unwrap_or(-1)).await
    }
    pub async fn stop(&mut self) -> Result<()> {
        if self.child.try_wait()?.is_none() {
            #[cfg(unix)]
            if let Some(id) = self
                .child
                .id()
                .and_then(|id| i32::try_from(id).ok())
                .and_then(rustix::process::Pid::from_raw)
            {
                match rustix::process::kill_process(id, rustix::process::Signal::TERM) {
                    Ok(()) | Err(rustix::io::Errno::SRCH) => (),
                    Err(e) => return Err(std::io::Error::from(e).into()),
                }
            }
            #[cfg(not(unix))]
            self.child.start_kill()?;
            match timeout(Duration::from_millis(1500), self.child.wait()).await {
                Ok(status) => {
                    status?;
                }
                Err(_) => {
                    self.child.start_kill()?;
                    timeout(Duration::from_secs(1), self.child.wait())
                        .await
                        .map_err(|_| {
                            invalid("SSH tunnel did not exit after forced termination")
                        })??;
                }
            }
        }
        if let Some(task) = self.stderr.take() {
            task.await
                .map_err(|e| invalid(format!("SSH stderr task failed: {e}")))??;
        }
        if let Some(directory) = self.directory.take() {
            directory.close()?;
        }
        Ok(())
    }
}
impl Drop for SshTunnel {
    fn drop(&mut self) {
        if let Some(task) = self.stderr.take() {
            task.abort();
        }
    }
}
