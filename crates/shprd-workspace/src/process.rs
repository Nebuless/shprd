use crate::{Error, Result};

use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Command,
};
#[derive(Debug, Clone, Default)]
pub enum HostConfig {
    #[default]
    Local,
    /// OpenSSH alias or user@host. Host keys must already be provisioned.
    Ssh { destination: String },
}
impl HostConfig {
    pub(crate) fn validate(&self) -> Result<()> {
        if let Self::Ssh { destination } = self {
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
            Self::Ssh { destination } => {
                let mut command = Command::new("ssh");
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
    let mut command = host.command(argv)?;
    if input.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command.spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::Process("missing stdout".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| Error::Process("missing stderr".into()))?;
    let stdin = child.stdin.take();
    let task = async {
        let write = async move {
            if let (Some(mut stream), Some(input)) = (stdin, input) {
                stream.write_all(input).await?;
                stream.shutdown().await?;
            }
            Ok::<_, std::io::Error>(())
        };
        let ((stdout, truncated), (stderr, _), _, status) = tokio::try_join!(
            bounded(stdout, limit),
            bounded(stderr, 8192),
            write,
            child.wait()
        )?;
        Ok::<_, Error>(Output {
            code: status.code().unwrap_or(-1),
            stdout,
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
            truncated,
        })
    };
    match tokio::time::timeout(Duration::from_secs(seconds), task).await {
        Ok(result) => result,
        Err(_) => {
            child.kill().await?;
            let _ = child.wait().await;
            Err(Error::Timeout)
        }
    }
}
pub(crate) async fn spool(
    host: &HostConfig,
    argv: &[&str],
    seconds: u64,
) -> Result<(tempfile::NamedTempFile, u64)> {
    let file = tempfile::NamedTempFile::new()?;
    let mut writer = tokio::fs::File::from_std(file.reopen()?);
    let mut child = host.command(argv)?.spawn()?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::Process("missing stdout".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| Error::Process("missing stderr".into()))?;
    let result = tokio::time::timeout(Duration::from_secs(seconds), async {
        let (size, (stderr, _), status) = tokio::try_join!(
            tokio::io::copy(&mut stdout, &mut writer),
            bounded(stderr, 8192),
            child.wait()
        )?;
        if !status.success() {
            return Err(Error::Process(
                String::from_utf8_lossy(&stderr).into_owned(),
            ));
        }
        writer.sync_all().await?;
        Ok((file, size))
    })
    .await;
    match result {
        Ok(value) => value,
        Err(_) => {
            child.kill().await?;
            let _ = child.wait().await;
            Err(Error::Timeout)
        }
    }
}
pub(crate) async fn stream_input<R: AsyncRead + Unpin>(
    host: &HostConfig,
    argv: &[&str],
    body: &mut R,
) -> Result<Output> {
    let mut command = host.command(argv)?;
    command.stdin(Stdio::piped());
    let mut child = command.spawn()?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| Error::Process("missing stdin".into()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::Process("missing stdout".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| Error::Process("missing stderr".into()))?;
    let result = tokio::time::timeout(Duration::from_secs(120), async {
        let copy = async move {
            tokio::io::copy(body, &mut stdin).await?;
            stdin.shutdown().await
        };
        let (_, (stdout, truncated), (stderr, _), status) = tokio::try_join!(
            copy,
            bounded(stdout, 16384),
            bounded(stderr, 8192),
            child.wait()
        )?;
        Ok(Output {
            code: status.code().unwrap_or(-1),
            stdout,
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
            truncated,
        })
    })
    .await;
    match result {
        Ok(value) => value,
        Err(_) => {
            child.kill().await?;
            let _ = child.wait().await;
            Err(Error::Timeout)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
                    destination: destination.into()
                }
                .command(&["git"])
                .is_err()
            );
        }
        let host = HostConfig::Ssh {
            destination: "user@host".into(),
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
        let output = stream_input(&HostConfig::Local, &["cat"], &mut &b"streamed"[..])
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
}
