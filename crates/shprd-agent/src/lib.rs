//! Authenticated attachment to existing runtimes. No engine launch or JSONL writes.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use image::{ImageFormat, ImageReader, Limits};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    io::{BufReader as StdBufReader, Cursor},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
const IMAGE_LIMIT: usize = 25 * 1024 * 1024;
const IMAGE_MAX_DIMENSION: u32 = 8192;
const IMAGE_MAX_DECODED_BYTES: u64 = 256 * 1024 * 1024;
const LIMIT: usize = IMAGE_LIMIT.div_ceil(3) * 4 + 1024 * 1024;

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
    #[error("invalid image attachment")]
    InvalidImage,
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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Capabilities {
    #[serde(default)]
    pub image_prompt: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub agent: Agent,
    pub name: String,
    pub cwd: String,
    pub connected: bool,
    pub busy: bool,
    #[serde(default)]
    pub capabilities: Capabilities,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Image {
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    pub data: String,
}

/// Reject unsupported, corrupted, and unsafe-sized inline rasters before extension dispatch.
pub fn validate_image(image: &Image) -> Result<(), Error> {
    let bytes = STANDARD.decode(&image.data).map_err(|_| Error::InvalidImage)?;
    validate_image_bytes(&image.mime_type, &bytes)
}

/// Validates declared MIME against decoded raster bytes with bounded decode resources.
pub fn validate_image_bytes(mime_type: &str, bytes: &[u8]) -> Result<(), Error> {
    if bytes.is_empty() || bytes.len() > IMAGE_LIMIT {
        return Err(Error::InvalidImage);
    }
    let expected = match mime_type {
        "image/png" => ImageFormat::Png,
        "image/jpeg" => ImageFormat::Jpeg,
        "image/gif" => ImageFormat::Gif,
        "image/webp" => ImageFormat::WebP,
        "image/bmp" => ImageFormat::Bmp,
        "image/x-icon" | "image/vnd.microsoft.icon" => ImageFormat::Ico,
        "image/avif" => ImageFormat::Avif,
        _ => return Err(Error::InvalidImage),
    };
    validate_container(expected, bytes)?;
    let mut reader = ImageReader::with_format(StdBufReader::new(Cursor::new(bytes)), expected);
    let mut limits = Limits::default();
    limits.max_image_width = Some(IMAGE_MAX_DIMENSION);
    limits.max_image_height = Some(IMAGE_MAX_DIMENSION);
    limits.max_alloc = Some(IMAGE_MAX_DECODED_BYTES);
    reader.limits(limits);
    reader.decode().map_err(|_| Error::InvalidImage)?;
    Ok(())
}

fn validate_container(format: ImageFormat, bytes: &[u8]) -> Result<(), Error> {
    let valid = match format {
        ImageFormat::Png => complete_png(bytes),
        ImageFormat::Jpeg => bytes.starts_with(&[0xff, 0xd8]) && bytes.ends_with(&[0xff, 0xd9]),
        ImageFormat::Gif => {
            (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"))
                && bytes.ends_with(&[0x3b])
        }
        ImageFormat::WebP => {
            bytes.len() >= 12
                && bytes.starts_with(b"RIFF")
                && bytes.get(8..12) == Some(b"WEBP")
                && u32::from_le_bytes(bytes[4..8].try_into().unwrap_or([0; 4])) as usize + 8
                    == bytes.len()
        }
        ImageFormat::Bmp => {
            bytes.len() >= 54
                && bytes.starts_with(b"BM")
                && u32::from_le_bytes(bytes[2..6].try_into().unwrap_or([0; 4])) as usize
                    == bytes.len()
        }
        ImageFormat::Ico => complete_ico(bytes),
        ImageFormat::Avif => complete_bmff(bytes),
        _ => false,
    };
    valid.then_some(()).ok_or(Error::InvalidImage)
}

fn complete_png(bytes: &[u8]) -> bool {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return false;
    }
    let mut offset: usize = 8;
    while offset.checked_add(12).is_some_and(|end| end <= bytes.len()) {
        let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap_or([0; 4]))
            as usize;
        let Some(end) = offset.checked_add(12 + length) else {
            return false;
        };
        if end > bytes.len() {
            return false;
        }
        let chunk = &bytes[offset + 4..offset + 8];
        let expected = u32::from_be_bytes(bytes[end - 4..end].try_into().unwrap_or([0; 4]));
        if png_crc32(&bytes[offset + 4..end - 4]) != expected {
            return false;
        }
        if chunk == b"IEND" {
            return length == 0 && end == bytes.len();
        }
        offset = end;
    }
    false
}

fn png_crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & (!((crc & 1).wrapping_sub(1))));
        }
    }
    !crc
}

fn complete_ico(bytes: &[u8]) -> bool {
    if bytes.len() < 6 || bytes[..4] != [0, 0, 1, 0] {
        return false;
    }
    let count = u16::from_le_bytes(bytes[4..6].try_into().unwrap_or([0; 2])) as usize;
    let Some(directory_end) = 6usize.checked_add(count.saturating_mul(16)) else {
        return false;
    };
    if count == 0 || directory_end > bytes.len() {
        return false;
    }
    (0..count).all(|index| {
        let entry = 6 + index * 16;
        let size = u32::from_le_bytes(bytes[entry + 8..entry + 12].try_into().unwrap_or([0; 4]))
            as usize;
        let offset =
            u32::from_le_bytes(bytes[entry + 12..entry + 16].try_into().unwrap_or([0; 4])) as usize;
        size > 0 && offset.checked_add(size).is_some_and(|end| end <= bytes.len())
    })
}

fn complete_bmff(bytes: &[u8]) -> bool {
    let mut offset: usize = 0;
    let mut first = true;
    while offset.checked_add(8).is_some_and(|end| end <= bytes.len()) {
        let mut length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap_or([0; 4]))
            as usize;
        let kind = &bytes[offset + 4..offset + 8];
        let header = if length == 1 {
            if offset + 16 > bytes.len() {
                return false;
            }
            length = u64::from_be_bytes(bytes[offset + 8..offset + 16].try_into().unwrap_or([0; 8]))
                as usize;
            16
        } else {
            8
        };
        if length < header || offset.checked_add(length).is_none_or(|end| end > bytes.len()) {
            return false;
        }
        if first && (kind != b"ftyp" || length < header + 4 || !matches!(&bytes[offset + header..offset + header + 4], b"avif" | b"avis")) {
            return false;
        }
        first = false;
        offset += length;
    }
    !first && offset == bytes.len()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    GetState,
    GetMessages,
    Prompt {
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        image: Option<Image>,
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
        if let Command::Prompt {
            image: Some(image),
            ..
        } = command
        {
            validate_image(image)?;
        }
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
