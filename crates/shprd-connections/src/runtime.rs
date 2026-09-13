use crate::{ConnectionId, Error, Profile, Result, error::invalid};
use serde::Serialize;
use std::{
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::{Arc, Mutex, MutexGuard, Weak},
};
use tokio::sync::watch;

pub type RuntimeFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;
pub type RuntimeFactory = Arc<dyn Fn(RuntimeContext) -> Result<Arc<dyn Runtime>> + Send + Sync>;
/// Start must be cancellation-safe; stop must retire every resource, including partial startup.
pub trait Runtime: Send + Sync {
    fn start<'a>(&'a self, context: &'a RuntimeContext) -> RuntimeFuture<'a, SocketPaths>;
    fn stop(&self) -> RuntimeFuture<'_, ()>;
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SocketPaths {
    pub control: PathBuf,
    pub render: PathBuf,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Disconnected,
    Connecting,
    Ready,
    Reconnecting,
    Stopping,
    Error,
}
#[derive(Clone, Debug, Serialize)]
pub struct Status {
    pub id: ConnectionId,
    pub label: String,
    pub source: String,
    pub is_default: bool,
    pub state: State,
    pub generation: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<StatusError>,
}
#[derive(Clone, Debug, Serialize)]
pub struct StatusError {
    pub message: String,
}
pub(crate) struct EntryData {
    pub profile: Profile,
    pub generation: u64,
    pub disconnect_revision: u64,
    pub state: State,
    pub error: Option<StatusError>,
    pub runtime: Option<Arc<dyn Runtime>>,
    pub paths: Option<SocketPaths>,
    pub factory: RuntimeFactory,
    pub registered: bool,
}
pub(crate) struct Entry {
    pub data: Mutex<EntryData>,
    pub operation: tokio::sync::Mutex<()>,
    pub cancel: watch::Sender<u64>,
}
pub(crate) fn lock<T>(m: &Mutex<T>) -> Result<MutexGuard<'_, T>> {
    m.lock()
        .map_err(|_| invalid("connection state lock poisoned"))
}
pub(crate) fn advance(data: &mut EntryData) -> Result<()> {
    data.generation = data
        .generation
        .checked_add(1)
        .filter(|g| *g <= 9_007_199_254_740_991)
        .ok_or_else(|| invalid("connection generation exhausted"))?;
    Ok(())
}
#[derive(Clone)]
pub struct RuntimeContext {
    pub(crate) entry: Weak<Entry>,
    pub(crate) generation: u64,
}
impl RuntimeContext {
    pub const fn generation(&self) -> u64 {
        self.generation
    }
    pub fn is_current(&self) -> bool {
        self.entry
            .upgrade()
            .and_then(|e| {
                lock(&e.data)
                    .ok()
                    .map(|d| d.registered && d.generation == self.generation)
            })
            .unwrap_or(false)
    }
    pub async fn cancelled(&self) {
        let Some(entry) = self.entry.upgrade() else {
            return;
        };
        let mut cancel = entry.cancel.subscribe();
        loop {
            if !self.is_current() {
                return;
            }
            if cancel.changed().await.is_err() {
                return;
            }
        }
    }
    /// Invalidates leases immediately. Host supervision retires runtime before retrying.
    pub fn report_error(&self, message: &str, reconnecting: bool) -> Result<bool> {
        let Some(entry) = self.entry.upgrade() else {
            return Ok(false);
        };
        let mut data = lock(&entry.data)?;
        if !data.registered || data.generation != self.generation {
            return Ok(false);
        }
        advance(&mut data)?;
        data.state = if reconnecting {
            State::Reconnecting
        } else {
            State::Error
        };
        data.error = Some(StatusError {
            message: sanitize_error(message),
        });
        entry.cancel.send_replace(data.generation);
        Ok(true)
    }
}
#[derive(Clone)]
pub struct Lease {
    pub connection_id: ConnectionId,
    pub paths: SocketPaths,
    pub(crate) context: RuntimeContext,
}
impl Lease {
    pub const fn generation(&self) -> u64 {
        self.context.generation
    }
    pub fn is_current(&self) -> bool {
        self.context
            .entry
            .upgrade()
            .and_then(|e| {
                lock(&e.data).ok().map(|d| {
                    d.registered
                        && d.state == State::Ready
                        && d.generation == self.context.generation
                })
            })
            .unwrap_or(false)
    }
    pub fn check(&self) -> Result<()> {
        if self.is_current() {
            Ok(())
        } else {
            Err(Error::Stale)
        }
    }
    /// Resolves when connection retirement invalidates this lease.
    pub async fn cancelled(&self) {
        self.context.cancelled().await;
    }
    /// Call after every awaited body read, before publishing that chunk.
    pub fn checked_chunk<T>(&self, chunk: T) -> Result<T> {
        self.check()?;
        Ok(chunk)
    }
}
pub fn sanitize_error(raw: &str) -> String {
    let patterns = [
        (r"(?i)([a-z][a-z0-9+.-]*://)([^@\s/]+)@", "${1}***@"),
        (
            r"(?i)\b((?:proxy-)?authorization)\s*:\s*(bearer|basic)\s+[^\s,;]+",
            "$1: $2 ***",
        ),
        (
            r#"(?i)(["']?)(access[_-]?token|refresh[_-]?token|api[_-]?key|password|passphrase|token|secret)["']?(\s*[:=]\s*)(?:"[^"]*"|'[^']*'|[^\s,;&}\]]+)"#,
            "$1$2$1$3***",
        ),
        (r"\x1b\[[0-?]*[ -/]*[@-~]", ""),
    ];
    let mut value = raw.to_owned();
    for (pattern, replacement) in patterns {
        if let Ok(regex) = regex::Regex::new(pattern) {
            value = regex.replace_all(&value, replacement).into_owned();
        }
    }
    value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(300)
        .collect()
}
