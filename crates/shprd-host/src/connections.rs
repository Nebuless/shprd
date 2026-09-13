//! Concrete local/SSH connection runtime owned by the native host.
use crate::{herdr, terminal};
use shprd_connections::{
    Error, ProbeResult, Profile, ProfileFactory, Result, Runtime, RuntimeContext, RuntimeFuture,
    SocketPaths, SshTunnel, Transport,
};
use std::{path::Path, sync::Arc, time::Duration};
use tokio::sync::Mutex;

struct NativeRuntime {
    profile: Profile,
    tunnel: Mutex<Option<SshTunnel>>,
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
                *slot = Some(SshTunnel::spawn(&self.profile, Path::new("ssh")).await?);
                let tunnel = slot
                    .as_mut()
                    .ok_or_else(|| Error::Invalid("missing SSH tunnel".into()))?;
                tunnel.wait_ready().await?;
                Ok(tunnel.paths().clone())
            }
        }
    }
}

impl Runtime for NativeRuntime {
    fn start<'a>(&'a self, _context: &'a RuntimeContext) -> RuntimeFuture<'a, SocketPaths> {
        Box::pin(async {
            let paths = self.open().await?;
            probe_paths(&paths).await?;
            Ok(paths)
        })
    }
    fn stop(&self) -> RuntimeFuture<'_, ()> {
        Box::pin(async {
            if let Some(mut tunnel) = self.tunnel.lock().await.take() {
                tunnel.stop().await?;
            }
            Ok(())
        })
    }
}

pub fn factory() -> ProfileFactory {
    Arc::new(|profile| {
        let profile = profile.clone();
        Arc::new(move |_| {
            Ok(Arc::new(NativeRuntime {
                profile: profile.clone(),
                tunnel: Mutex::new(None),
            }))
        })
    })
}

pub async fn probe(profile: Profile) -> Result<ProbeResult> {
    let runtime = NativeRuntime {
        profile,
        tunnel: Mutex::new(None),
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
