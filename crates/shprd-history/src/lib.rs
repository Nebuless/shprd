//! Readonly projection of supported native agent session formats.
mod access;
mod atif;
mod build;
mod cache;
mod claude;
mod codex;
mod error;
mod grok;
mod history;
mod json;
mod kimi;
mod pi;
mod project;
mod remote;
mod resolve;
mod service;
mod summary;
mod types;

pub use access::LocalFiles;
pub use error::{Error, Result};
pub use history::{Cursor, Entry, Message, Projection, Update, WINDOW_LIMIT};
pub use remote::RemoteFiles;
pub use resolve::Resolver;
pub use service::HistoryService;
pub use summary::{MAX_PREVIEW_BYTES, SessionStats, SessionView, TokenUsage};
pub use types::{
    Agent, FileMeta, HostConfig, PaneMetadata, ResolveStatus, ResolvedSession, SessionKind,
    SessionRef,
};
