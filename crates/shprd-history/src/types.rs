use crate::{Result, error::invalid};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Agent {
    Codex,
    Claude,
    Kimi,
    Grok,
    Pi,
}
impl Agent {
    pub const ALL: [Self; 5] = [Self::Codex, Self::Claude, Self::Kimi, Self::Grok, Self::Pi];
    pub const fn canonical_name(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Kimi => "kimi",
            Self::Grok => "grok",
            Self::Pi => "pi",
        }
    }
    pub const fn atif_name(self) -> &'static str {
        match self {
            Self::Claude => "claude-code",
            Self::Kimi => "kimi-code",
            Self::Grok => "grok-build",
            _ => self.canonical_name(),
        }
    }
    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "codex" => Ok(Self::Codex),
            "claude" | "claude-code" => Ok(Self::Claude),
            "kimi" | "kimi-code" | "kimi code" => Ok(Self::Kimi),
            "grok" | "grok-build" | "grok build" => Ok(Self::Grok),
            "pi" | "pi-agent" | "pi-coding-agent" => Ok(Self::Pi),
            other => Err(crate::Error::UnsupportedAgent(other.into())),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionKind {
    Id,
    Path,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRef {
    pub source: String,
    pub agent: Agent,
    pub kind: SessionKind,
    pub value: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileMeta {
    pub path: PathBuf,
    pub mtime_ms: u64,
    pub size: Option<u64>,
    pub identity: Option<String>,
    pub change_token: Option<String>,
    pub session_id: Option<String>,
    pub created_at_ms: Option<u64>,
    pub model_name: Option<String>,
    pub agent_version: Option<String>,
}
impl FileMeta {
    pub fn fixture(path: impl Into<PathBuf>, text: &str, mtime_ms: u64) -> Self {
        Self {
            path: path.into(),
            mtime_ms,
            size: Some(u64::try_from(text.len()).unwrap_or(u64::MAX)),
            identity: None,
            change_token: None,
            session_id: None,
            created_at_ms: None,
            model_name: None,
            agent_version: None,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneMetadata {
    pub pane_id: String,
    pub workspace_id: String,
    pub tab_id: String,
    pub agent: Agent,
    pub session: Option<SessionRef>,
    pub foreground_cwd: Option<PathBuf>,
    pub reported_path: PathBuf,
}
impl PaneMetadata {
    pub fn new(
        pane_id: impl Into<String>,
        workspace_id: impl Into<String>,
        tab_id: impl Into<String>,
        agent: Agent,
        path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            pane_id: pane_id.into(),
            workspace_id: workspace_id.into(),
            tab_id: tab_id.into(),
            agent,
            session: None,
            foreground_cwd: None,
            reported_path: path.into(),
        }
    }
    pub fn validated(&self) -> Result<()> {
        if self.pane_id.is_empty() {
            return Err(invalid("agent session requires pane_id"));
        }
        if self.reported_path.as_os_str().is_empty() {
            return Err(invalid("agent session path is required"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolveStatus {
    MissingSession,
    MissingFile,
    Ok,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedSession {
    pub version: u8,
    pub agent: Agent,
    pub pane_id: String,
    pub workspace_id: String,
    pub tab_id: String,
    pub status: ResolveStatus,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    pub updated_at: String,
    pub path: String,
    pub session: Option<SessionRef>,
    pub file: Option<FileMeta>,
}
impl ResolvedSession {
    pub fn missing(pane: &PaneMetadata, status: ResolveStatus, detail: impl Into<String>) -> Self {
        Self {
            version: 1,
            agent: pane.agent,
            pane_id: pane.pane_id.clone(),
            workspace_id: pane.workspace_id.clone(),
            tab_id: pane.tab_id.clone(),
            status,
            detail: detail.into(),
            command: Some(format!(
                "herdr integration install {}",
                pane.agent.canonical_name()
            )),
            updated_at: "1970-01-01T00:00:00.000Z".into(),
            path: String::new(),
            session: pane.session.clone(),
            file: None,
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct HostConfig {
    pub home: Option<PathBuf>,
    pub pi_agent_dir: Option<PathBuf>,
    pub grok_home: Option<PathBuf>,
    pub search_depth: usize,
    pub search_files: usize,
    pub cache_entries: usize,
    pub cache_bytes: usize,
}
impl HostConfig {
    pub fn bounded(mut self) -> Self {
        if self.search_depth == 0 {
            self.search_depth = 8;
        }
        if self.search_files == 0 {
            self.search_files = 200;
        }
        if self.cache_entries == 0 {
            self.cache_entries = 16;
        }
        if self.cache_bytes == 0 {
            self.cache_bytes = 32 * 1024 * 1024;
        }
        self
    }
}
