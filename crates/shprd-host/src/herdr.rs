//! One-shot Herdr control requests and separate event subscriptions.

use serde_json::Value;
use std::{path::Path, time::Duration};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};

const MAX_LINE_BYTES: usize = 1024 * 1024;

#[cfg(unix)]
pub(crate) type Socket = tokio::net::UnixStream;
#[cfg(windows)]
pub(crate) type Socket = tokio::net::windows::named_pipe::NamedPipeClient;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Herdr socket: {0}")]
    Io(#[from] std::io::Error),
    #[error("timeout: {0}")]
    Timeout(String),
    #[error("Herdr response line is too large")]
    LineTooLarge,
    #[error("connection closed before response")]
    Closed,
    #[error("bad JSON from Herdr: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid response envelope from Herdr")]
    Envelope,
    #[error("{code}: {message}")]
    Remote { code: String, message: String },
}

pub(crate) async fn connect(path: &Path) -> Result<Socket, Error> {
    #[cfg(unix)]
    let socket = Socket::connect(path).await?;
    #[cfg(windows)]
    let socket = tokio::net::windows::named_pipe::ClientOptions::new().open(path)?;
    Ok(socket)
}

async fn read_line(
    reader: &mut (impl AsyncBufRead + Unpin),
    line: &mut Vec<u8>,
) -> Result<(), Error> {
    loop {
        let bytes = reader.fill_buf().await?;
        if bytes.is_empty() {
            return Err(Error::Closed);
        }
        let newline = bytes.iter().position(|byte| *byte == b'\n');
        let length = newline.unwrap_or(bytes.len());
        if length > MAX_LINE_BYTES.saturating_sub(line.len()) {
            return Err(Error::LineTooLarge);
        }
        line.extend_from_slice(&bytes[..length]);
        reader.consume(length + usize::from(newline.is_some()));
        if newline.is_some() {
            return Ok(());
        }
    }
}

fn response(line: &[u8], id: &str) -> Result<Value, Error> {
    let value: Value = serde_json::from_slice(line)?;
    let envelope = value.as_object().ok_or(Error::Envelope)?;
    if envelope.get("id").and_then(Value::as_str) != Some(id)
        || envelope.contains_key("result") == envelope.contains_key("error")
    {
        return Err(Error::Envelope);
    }
    if let Some(error) = envelope.get("error") {
        let error = error.as_object().ok_or(Error::Envelope)?;
        return Err(Error::Remote {
            code: error
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or("error")
                .to_owned(),
            message: error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Herdr request failed")
                .to_owned(),
        });
    }
    Ok(envelope.get("result").ok_or(Error::Envelope)?.clone())
}

/// Send one request on a fresh socket; completion or cancellation drops the socket.
pub async fn call(
    path: &Path,
    method: &str,
    params: &Value,
    deadline: Duration,
) -> Result<Value, Error> {
    tokio::time::timeout(deadline, async {
        let mut socket = connect(path).await?;
        let mut request =
            serde_json::to_vec(&serde_json::json!({"id":"rpc", "method":method, "params":params}))?;
        request.push(b'\n');
        socket.write_all(&request).await?;
        let mut reader = BufReader::new(socket);
        let mut line = Vec::new();
        read_line(&mut reader, &mut line).await?;
        response(&line, "rpc")
    })
    .await
    .map_err(|_| Error::Timeout(method.to_owned()))?
}

/// An acknowledged event stream. Dropping it closes its socket.
#[derive(Debug)]
pub struct Subscription {
    reader: BufReader<Socket>,
    line: Vec<u8>,
}

impl Subscription {
    pub async fn open(path: &Path, selectors: &Value, deadline: Duration) -> Result<Self, Error> {
        tokio::time::timeout(deadline, async {
            let mut socket = connect(path).await?;
            let mut request = serde_json::to_vec(&serde_json::json!({
                "id":"sub", "method":"events.subscribe", "params":{"subscriptions":selectors}
            }))?;
            request.push(b'\n');
            socket.write_all(&request).await?;
            let mut reader = BufReader::new(socket);
            let mut line = Vec::new();
            loop {
                read_line(&mut reader, &mut line).await?;
                if !line.iter().all(u8::is_ascii_whitespace) {
                    break;
                }
                line.clear();
            }
            response(&line, "sub")?;
            line.clear();
            Ok(Self { reader, line })
        })
        .await
        .map_err(|_| Error::Timeout("events.subscribe".to_owned()))?
    }

    /// Wait for an event. Partial input survives cancellation of this future.
    pub async fn next(&mut self) -> Result<Value, Error> {
        loop {
            read_line(&mut self.reader, &mut self.line).await?;
            let value = serde_json::from_slice::<Value>(&self.line);
            self.line.clear();
            if let Ok(value) = value {
                if value.get("event").is_some_and(Value::is_string) {
                    return Ok(value);
                }
            }
        }
    }
}
