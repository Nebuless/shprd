use crate::{Agent, Error, FileMeta, HostConfig, Result, error::invalid};
use std::path::{Path, PathBuf};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    time::{Duration, timeout},
};

const REMOTE_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_REMOTE_BYTES: usize = 2 * 1024 * 1024;

/// Readonly SSH adapter. All paths are supplied by host code; no session JSONL is changed.
pub struct RemoteFiles {
    host: String,
    executable: PathBuf,
}

impl RemoteFiles {
    pub fn new(
        host: impl Into<String>,
        executable: impl Into<PathBuf>,
        _config: HostConfig,
    ) -> Result<Self> {
        let host = host.into();
        validate_host(&host)?;
        Ok(Self {
            host,
            executable: executable.into(),
        })
    }

    pub async fn metadata(&self, path: &Path) -> Result<Option<FileMeta>> {
        let path = absolute_path(path, "remote session path")?;
        let script = format!(
            "set -eu\npath={}\n[ -f \"$path\" ] || exit 44\nsize=$(stat -c %s \"$path\" 2>/dev/null || stat -f %z \"$path\")\nmtime=$(stat -c %Y \"$path\" 2>/dev/null || stat -f %m \"$path\")\nprintf '%s\\t%s' \"$size\" \"$mtime\"",
            quote(&path)
        );
        let output = self.command(&script).await?;
        if output.status.code() == Some(44) {
            return Ok(None);
        }
        if !output.status.success() {
            return Err(remote_error(&output));
        }
        let fields = String::from_utf8(output.stdout)
            .map_err(|_| invalid("remote session metadata is not UTF-8"))?
            .split('\t')
            .map(str::trim)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let [size, mtime] = fields.as_slice() else {
            return Err(invalid("remote session metadata was malformed"));
        };
        let size = size
            .parse()
            .map_err(|_| invalid("remote session size was malformed"))?;
        let mtime_seconds: u64 = mtime
            .parse()
            .map_err(|_| invalid("remote session mtime was malformed"))?;
        Ok(Some(FileMeta {
            path: PathBuf::from(path),
            mtime_ms: mtime_seconds.saturating_mul(1000),
            size: Some(size),
            identity: None,
            change_token: None,
            session_id: None,
            created_at_ms: None,
            model_name: None,
            agent_version: None,
        }))
    }

    pub async fn read_text(&self, path: &Path) -> Result<String> {
        String::from_utf8(self.read(path, None).await?)
            .map_err(|_| invalid("remote session file is not UTF-8"))
    }

    pub async fn read_prefix(&self, path: &Path, limit: usize) -> Result<Vec<u8>> {
        self.read(path, Some(limit.max(1))).await
    }

    pub async fn find_session(
        &self,
        agent: Agent,
        root: &Path,
        id: &str,
        depth: usize,
        files: usize,
    ) -> Result<Option<PathBuf>> {
        let root = absolute_path(root, "remote session root")?;
        if !safe_id(id) {
            return Err(invalid("session id contains path traversal"));
        }
        let matcher = match agent {
            Agent::Codex => "case \"$candidate\" in *\"$id\"*.jsonl) ;; *) continue ;; esac",
            Agent::Claude => "case \"$candidate\" in */\"$id\".jsonl) ;; *) continue ;; esac",
            Agent::Kimi => {
                "case \"$candidate\" in */\"$id\"/agents/main/wire.jsonl) ;; *) continue ;; esac"
            }
            Agent::Grok => {
                "case \"$candidate\" in *\"$id\"*/chat_history.jsonl) ;; *) continue ;; esac"
            }
            Agent::Pi => {
                "case \"$candidate\" in */\"$id\".jsonl|*_\"$id\".jsonl) ;; *) continue ;; esac"
            }
        };
        let depth = depth.clamp(1, 8);
        let files = files.clamp(1, 200);
        let script = format!(
            "set -eu\nroot={}\nid={}\n[ -d \"$root\" ] || exit 44\nlatest=\"\"\nlatest_mtime=0\ncount=0\nwhile IFS= read -r -d '' candidate; do\n  count=$((count + 1))\n  [ \"$count\" -le {files} ] || break\n  {matcher}\n  mtime=$(stat -c %Y \"$candidate\" 2>/dev/null || stat -f %m \"$candidate\")\n  if [ \"$mtime\" -ge \"$latest_mtime\" ]; then latest=\"$candidate\"; latest_mtime=\"$mtime\"; fi\ndone < <(find \"$root\" -maxdepth {depth} -type f -name '*.jsonl' -print0)\n[ -n \"$latest\" ] || exit 44\nprintf '%s' \"$latest\"",
            quote(&root),
            quote(id),
        );
        let output = self.command(&script).await?;
        if output.status.code() == Some(44) {
            return Ok(None);
        }
        if !output.status.success() {
            return Err(remote_error(&output));
        }
        let path = String::from_utf8(output.stdout)
            .map_err(|_| invalid("remote session path is not UTF-8"))?;
        Ok((!path.is_empty()).then_some(PathBuf::from(path)))
    }

    async fn read(&self, path: &Path, prefix: Option<usize>) -> Result<Vec<u8>> {
        let path = absolute_path(path, "remote session path")?;
        let command = match prefix {
            Some(limit) => format!("head -c {limit} {}", quote(&path)),
            None => format!("cat {}", quote(&path)),
        };
        let output = self.command(&format!("set -eu\n{command}")).await?;
        if !output.status.success() {
            return Err(remote_error(&output));
        }
        Ok(output.stdout)
    }

    async fn command(&self, script: &str) -> Result<std::process::Output> {
        let remote_command = format!("bash -lc {}", quote(script));
        let arguments = [
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
            "--",
            self.host.as_str(),
            remote_command.as_str(),
        ];
        timeout(REMOTE_TIMEOUT, async {
            let mut child = Command::new(&self.executable)
                .args(arguments)
                .env("LC_ALL", "C")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .kill_on_drop(true)
                .spawn()?;
            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| invalid("missing SSH stdout"))?;
            let stderr = child
                .stderr
                .take()
                .ok_or_else(|| invalid("missing SSH stderr"))?;
            let (stdout, stderr, status) = tokio::try_join!(
                read_bounded(stdout, MAX_REMOTE_BYTES),
                read_bounded(stderr, 64 * 1024),
                async { child.wait().await.map_err(Error::from) },
            )?;
            Ok(std::process::Output {
                status,
                stdout,
                stderr,
            })
        })
        .await
        .map_err(|_| invalid("remote session command timed out"))?
    }
}

async fn read_bounded(reader: impl AsyncRead + Unpin, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() > limit {
        return Err(invalid("remote session output exceeds read limit"));
    }
    Ok(bytes)
}

fn absolute_path(path: &Path, label: &str) -> Result<String> {
    if !path.is_absolute() {
        return Err(invalid(format!("{label} must be absolute")));
    }
    Ok(path.to_string_lossy().into())
}

fn safe_id(value: &str) -> bool {
    !value.is_empty() && value != "." && value != ".." && !value.contains(['/', '\\', '\0'])
}

fn validate_host(host: &str) -> Result<()> {
    if host.is_empty()
        || host.len() > 320
        || host.starts_with('-')
        || host
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || b"/:,=".contains(&byte))
    {
        return Err(invalid(
            "ssh destination must be an OpenSSH alias or user@host",
        ));
    }
    let parts = host.split('@').collect::<Vec<_>>();
    let valid = |value: &str| {
        !value.is_empty()
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    };
    if !match parts.as_slice() {
        [one] => valid(one),
        [user, name] => valid(user) && valid(name),
        _ => false,
    } {
        return Err(invalid(
            "ssh destination must be an OpenSSH alias or user@host",
        ));
    }
    Ok(())
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace(0x27 as char, "'\\''"))
}

fn remote_error(output: &std::process::Output) -> Error {
    let detail = String::from_utf8_lossy(&output.stderr)
        .trim()
        .chars()
        .take(1000)
        .collect::<String>();
    Error::Remote(if detail.is_empty() {
        format!(
            "remote session command exited {}",
            output.status.code().unwrap_or(-1)
        )
    } else {
        detail
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn output_limit_fails_before_stream_closes() {
        let (mut writer, reader) = tokio::io::duplex(32);
        writer.write_all(b"123456789").await.unwrap();
        let result = timeout(Duration::from_secs(1), read_bounded(reader, 8))
            .await
            .expect("oversized stream must fail without EOF");
        assert!(matches!(result, Err(Error::Invalid(_))));
        drop(writer);
    }
}
