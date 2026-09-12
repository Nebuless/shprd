//! Authenticated attachment to existing runtimes. No engine launch or JSONL writes.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
const LIMIT: usize = 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("attachment I/O failed")]
    Io(#[from] std::io::Error),
    #[error("invalid attachment JSON")]
    Json(#[from] serde_json::Error),
    #[error("attachment deadline exceeded")]
    Timeout(#[from] tokio::time::error::Elapsed),
    #[error("unsafe attachment discovery")]
    UnsafeDiscovery,
    #[error("attachment not found")]
    NotFound,
    #[error("attachment frame exceeds limit")]
    FrameTooLarge,
    #[error("attachment protocol mismatch")]
    Protocol,
    #[error("native command rejected: {0}")]
    Native(String),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Agent {
    Senpi,
    Atomic,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub agent: Agent,
    pub name: String,
    pub cwd: String,
    pub connected: bool,
    pub busy: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    GetState,
    GetMessages,
    Prompt {
        message: String,
    },
    Abort,
    SetModel {
        provider: String,
        #[serde(rename = "modelId")]
        model_id: String,
    },
    GetAvailableModels,
    UiDialog {
        #[serde(flatten)]
        dialog: Dialog,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Dialog {
    Confirm {
        title: String,
        message: String,
    },
    Input {
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placeholder: Option<String>,
    },
    Select {
        title: String,
        options: Vec<String>,
    },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Discovery {
    version: u8,
    session_id: String,
    agent: Agent,
    endpoint: String,
    token: String,
}
#[cfg(unix)]
type Stream = tokio::net::UnixStream;
#[cfg(windows)]
type Stream = tokio::net::windows::named_pipe::NamedPipeClient;

/// One socket to an existing engine. Drop disconnects without stopping its engine.
pub struct Attachment {
    stream: BufReader<Stream>,
    discovery: Discovery,
    sequence: u64,
}

async fn private(path: &Path, directory: bool) -> Result<(), Error> {
    let metadata = tokio::fs::symlink_metadata(path).await?;
    if metadata.file_type().is_symlink() || metadata.is_dir() != directory {
        return Err(Error::UnsafeDiscovery);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != rustix::process::geteuid().as_raw() || metadata.mode() & 0o077 != 0 {
            return Err(Error::UnsafeDiscovery);
        }
    }
    Ok(())
}
async fn discover(directory: &Path) -> Result<Vec<Discovery>, Error> {
    private(directory, true).await?;
    let mut entries = tokio::fs::read_dir(directory).await?;
    let mut discoveries = Vec::new();
    let mut count = 0;
    while let Some(entry) = entries.next_entry().await? {
        count += 1;
        if count > 256 {
            return Err(Error::UnsafeDiscovery);
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.len() != 32 || !name.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            continue;
        }
        let owner = entry.path();
        private(&owner, true).await?;
        let path = owner.join("attachment.json");
        match private(&path, false).await {
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => continue,
            result => result?,
        }
        let mut bytes = Vec::new();
        tokio::fs::File::open(&path)
            .await?
            .take(4097)
            .read_to_end(&mut bytes)
            .await?;
        if bytes.len() > 4096 {
            return Err(Error::UnsafeDiscovery);
        }
        let descriptor: Discovery = serde_json::from_slice(&bytes)?;
        let prefix = match descriptor.agent {
            Agent::Senpi => "senpi:",
            Agent::Atomic => "atomic:",
        };
        if descriptor.version != 1
            || !descriptor.session_id.starts_with(prefix)
            || descriptor.session_id.len() > 256
            || descriptor.token.len() != 64
            || !descriptor
                .token
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(Error::UnsafeDiscovery);
        }
        #[cfg(unix)]
        {
            if Path::new(&descriptor.endpoint) != owner.join("control.sock") {
                return Err(Error::UnsafeDiscovery);
            }
            match private(Path::new(&descriptor.endpoint), false).await {
                Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => continue,
                result => result?,
            }
            use std::os::unix::fs::FileTypeExt;
            if !tokio::fs::symlink_metadata(&descriptor.endpoint)
                .await?
                .file_type()
                .is_socket()
            {
                return Err(Error::UnsafeDiscovery);
            }
        }
        #[cfg(windows)]
        {
            let expected = format!(r"\\.\pipe\shprd-{name}-");
            if !descriptor.endpoint.starts_with(&expected)
                || descriptor.endpoint.len() != expected.len() + 16
            {
                return Err(Error::UnsafeDiscovery);
            }
        }
        discoveries.push(descriptor);
    }
    Ok(discoveries)
}
impl Attachment {
    async fn open(discovery: Discovery) -> Result<Self, Error> {
        #[cfg(unix)]
        let stream =
            tokio::time::timeout(Duration::from_secs(3), Stream::connect(&discovery.endpoint))
                .await??;
        #[cfg(windows)]
        let stream =
            tokio::net::windows::named_pipe::ClientOptions::new().open(&discovery.endpoint)?;
        Ok(Self {
            stream: BufReader::new(stream),
            discovery,
            sequence: 0,
        })
    }
    /// Authenticate discovery against the live runtime. No mutation retry.
    pub async fn connect(directory: &Path, session_id: &str) -> Result<Self, Error> {
        let descriptor = discover(directory)
            .await?
            .into_iter()
            .find(|item| item.session_id == session_id)
            .ok_or(Error::NotFound)?;
        let mut attachment = Self::open(descriptor).await?;
        attachment.state().await?;
        Ok(attachment)
    }
    async fn frame(&mut self) -> Result<Value, Error> {
        let mut line = Vec::new();
        let size = (&mut self.stream)
            .take(u64::try_from(LIMIT + 1).map_err(|_| Error::FrameTooLarge)?)
            .read_until(b'\n', &mut line)
            .await?;
        if size > LIMIT {
            return Err(Error::FrameTooLarge);
        }
        if line.last() != Some(&b'\n') {
            return Err(Error::Protocol);
        }
        Ok(serde_json::from_slice(&line)?)
    }
    fn valid_event(&self, frame: &Value) -> bool {
        frame.get("agent_event").is_some_and(|event| {
            event["session_id"] == self.discovery.session_id && event.get("event").is_some()
        })
    }
    async fn state(&mut self) -> Result<Session, Error> {
        let result = self.request(&Command::GetState, |_| {}).await?;
        let state: Session = serde_json::from_value(result)?;
        if state.id != self.discovery.session_id
            || state.agent != self.discovery.agent
            || !state.connected
        {
            return Err(Error::Protocol);
        }
        Ok(state)
    }
    /// Normalized result; event callback receives full agent_event envelopes.
    /// A failed mutation may already be delivered: reconnect, never replay it.
    pub async fn request(
        &mut self,
        command: &Command,
        mut event: impl FnMut(Value),
    ) -> Result<Value, Error> {
        self.sequence = self.sequence.checked_add(1).ok_or(Error::Protocol)?;
        let id = self.sequence.to_string();
        let mut wire = serde_json::to_vec(
            &json!({"id":id,"token":self.discovery.token,"session_id":self.discovery.session_id,"command":command}),
        )?;
        wire.push(b'\n');
        if wire.len() > LIMIT {
            return Err(Error::FrameTooLarge);
        }
        tokio::time::timeout(Duration::from_secs(35), async {
            self.stream.get_mut().write_all(&wire).await?;
            loop {
                let frame = self.frame().await?;
                if self.valid_event(&frame) {
                    event(frame);
                    continue;
                }
                if frame["id"] != id {
                    return Err(Error::Protocol);
                }
                if let Some(error) = frame.get("error") {
                    let code = error["code"]
                        .as_str()
                        .filter(|code| {
                            code.len() <= 64
                                && code
                                    .bytes()
                                    .all(|byte| byte.is_ascii_uppercase() || byte == b'_')
                        })
                        .ok_or(Error::Protocol)?;
                    return Err(Error::Native(code.to_owned()));
                }
                return frame.get("result").cloned().ok_or(Error::Protocol);
            }
        })
        .await?
    }
    /// Caller owns cancellation, reconnect, and snapshot refresh after event gaps.
    pub async fn next_event(&mut self) -> Result<Value, Error> {
        let frame = self.frame().await?;
        if !self.valid_event(&frame) {
            return Err(Error::Protocol);
        }
        Ok(frame)
    }
}
/// agent_control.list: only authenticated, reachable owners.
pub async fn list(directory: &Path) -> Result<Value, Error> {
    let discoveries = match discover(directory).await {
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(json!({"sessions":[]}));
        }
        result => result?,
    };
    let mut sessions = Vec::new();
    for descriptor in discoveries {
        let state = tokio::time::timeout(Duration::from_secs(3), async {
            Attachment::open(descriptor).await?.state().await
        })
        .await;
        if let Ok(Ok(state)) = state {
            sessions.push(state);
        }
    }
    Ok(json!({"sessions":sessions}))
}
/// agent_control.request: route by authenticated session identity, never file path.
pub async fn request(
    directory: &Path,
    session_id: &str,
    command: &Command,
    event: impl FnMut(Value),
) -> Result<Value, Error> {
    Attachment::connect(directory, session_id)
        .await?
        .request(command, event)
        .await
}
/// Matches the extension default; callers may supply an explicit private directory.
pub fn default_directory() -> Result<PathBuf, Error> {
    if let Some(path) = std::env::var_os("SHPRD_AGENT_DIR") {
        return Ok(path.into());
    }
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .ok_or(Error::NotFound)?;
    Ok(PathBuf::from(home).join(".shprd").join("attachments"))
}
