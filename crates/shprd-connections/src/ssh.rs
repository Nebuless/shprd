use crate::{
    Profile, Result, Transport,
    error::invalid,
    paths::{validate_destination, validate_remote_path},
};
use serde::Serialize;
const NONINTERACTIVE: [&str; 18] = [
    "-o",
    "BatchMode=yes",
    "-o",
    "StrictHostKeyChecking=yes",
    "-o",
    "PermitLocalCommand=no",
    "-o",
    "RequestTTY=no",
    "-o",
    "ControlMaster=no",
    "-o",
    "ControlPath=none",
    "-o",
    "ControlPersist=no",
    "-o",
    "ConnectTimeout=8",
    "-o",
    "ConnectionAttempts=1",
];
pub fn ssh_command_argv(destination: &str, command: &str) -> Result<Vec<String>> {
    validate_destination(destination)?;
    Ok(NONINTERACTIVE
        .into_iter()
        .chain(["--", destination, command])
        .map(str::to_owned)
        .collect())
}
pub fn ssh_tunnel_argv(destination: &str, forwards: &[(&str, &str)]) -> Result<Vec<String>> {
    validate_destination(destination)?;
    let mut argv: Vec<_> = NONINTERACTIVE
        .into_iter()
        .chain([
            "-o",
            "ExitOnForwardFailure=yes",
            "-o",
            "ServerAliveInterval=20",
            "-o",
            "ServerAliveCountMax=3",
            "-o",
            "StreamLocalBindUnlink=yes",
            "-o",
            "StreamLocalBindMask=0177",
            "-N",
        ])
        .map(str::to_owned)
        .collect();
    for (local, remote) in forwards {
        validate_remote_path(local)?;
        validate_remote_path(remote)?;
        if local.is_empty() || remote.is_empty() {
            return Err(invalid("SSH forwarding paths must not be empty"));
        }
        argv.extend(["-L".into(), format!("{local}:{remote}")]);
    }
    argv.extend(["--".into(), destination.into()]);
    Ok(argv)
}
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SshFailureKind {
    Authentication,
    HostKey,
    Unreachable,
    Unsupported,
    Exited,
}
#[derive(Clone, Debug, thiserror::Error, Serialize)]
#[error("{message}")]
pub struct SshFailure {
    pub message: String,
    pub retryable: bool,
    pub kind: SshFailureKind,
    pub exit_code: i32,
}
pub fn classify_ssh_failure(exit_code: i32, stderr: &str) -> SshFailure {
    let diagnostic = stderr.to_lowercase();
    let (kind, retryable, message) = if [
        "remote host identification has changed",
        "host key verification failed",
        "no matching host key type found",
    ]
    .iter()
    .any(|p| diagnostic.contains(p))
    {
        (
            SshFailureKind::HostKey,
            false,
            "SSH host-key verification failed; verify the host outside Herdr Studio".into(),
        )
    } else if [
        "permission denied",
        "no supported authentication methods",
        "too many authentication failures",
    ]
    .iter()
    .any(|p| diagnostic.contains(p))
    {
        (
            SshFailureKind::Authentication,
            false,
            "SSH authentication failed; verify the service user's OpenSSH agent and config".into(),
        )
    } else if [
        "connection timed out",
        "connection refused",
        "no route to host",
        "could not resolve hostname",
        "operation timed out",
    ]
    .iter()
    .any(|p| diagnostic.contains(p))
    {
        (
            SshFailureKind::Unreachable,
            true,
            "SSH destination is temporarily unreachable".into(),
        )
    } else {
        (
            SshFailureKind::Exited,
            true,
            format!("SSH tunnel exited unexpectedly (code {exit_code})"),
        )
    };
    SshFailure {
        message,
        retryable,
        kind,
        exit_code,
    }
}
impl From<SshFailure> for crate::Error {
    fn from(e: SshFailure) -> Self {
        Self::Runtime {
            message: e.message,
            retryable: e.retryable,
        }
    }
}
pub(crate) fn ssh_profile(profile: &Profile) -> Result<(&str, &str, &str)> {
    match profile.transport() {
        Transport::Ssh {
            ssh_destination,
            remote_control_socket_path,
            remote_client_socket_path,
        } => Ok((
            ssh_destination,
            remote_control_socket_path,
            remote_client_socket_path,
        )),
        Transport::Local { .. } => Err(invalid("SSH profile required")),
    }
}
