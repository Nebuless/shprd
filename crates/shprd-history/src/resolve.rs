use crate::{
    Agent, FileMeta, HostConfig, PaneMetadata, ResolveStatus, ResolvedSession, Result, SessionKind,
    error::invalid,
};
use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
};

pub struct Resolver {
    config: HostConfig,
}
impl Resolver {
    pub fn new(config: HostConfig) -> Self {
        Self {
            config: config.bounded(),
        }
    }
    pub fn resolve(&self, pane: &PaneMetadata) -> Result<ResolvedSession> {
        pane.validated()?;
        let Some(session) = pane.session.clone() else {
            return Ok(ResolvedSession::missing(
                pane,
                ResolveStatus::MissingSession,
                "Herdr has not received an agent session id for this pane.",
            ));
        };
        let candidate = match session.kind {
            SessionKind::Path => Some(PathBuf::from(&session.value)),
            SessionKind::Id => self.find_id(pane.agent, &session.value)?,
        };
        let Some(path) = candidate else {
            return Ok(ResolvedSession::missing(
                pane,
                ResolveStatus::MissingFile,
                format!(
                    "Could not find the {} session transcript for {}.",
                    pane.agent.canonical_name(),
                    session.value
                ),
            ));
        };
        let Some(file) = file_meta(&path)? else {
            return Ok(ResolvedSession::missing(
                pane,
                ResolveStatus::MissingFile,
                "session transcript is unavailable",
            ));
        };
        Ok(ResolvedSession {
            version: 1,
            agent: pane.agent,
            pane_id: pane.pane_id.clone(),
            workspace_id: pane.workspace_id.clone(),
            tab_id: pane.tab_id.clone(),
            status: ResolveStatus::Ok,
            detail: String::new(),
            command: None,
            updated_at: file.mtime_ms.to_string(),
            path: path.to_string_lossy().into(),
            session: Some(session),
            file: Some(file),
        })
    }
    fn find_id(&self, agent: Agent, id: &str) -> Result<Option<PathBuf>> {
        if !safe_id(id) {
            return Err(invalid("session id contains path traversal"));
        }
        let root = self.root(agent);
        let mut newest: Option<(PathBuf, FileMeta)> = None;
        for path in walk(
            &root,
            self.config.search_depth,
            self.config.search_files,
            |path| matches_agent(agent, path, id),
        ) {
            let Some(meta) = file_meta(&path)? else {
                continue;
            };
            if newest
                .as_ref()
                .is_none_or(|(_, old)| meta.mtime_ms > old.mtime_ms)
            {
                newest = Some((path, meta));
            }
        }
        Ok(newest.map(|(path, _)| path))
    }
    fn root(&self, agent: Agent) -> PathBuf {
        let home = self
            .config
            .home
            .clone()
            .unwrap_or_else(|| PathBuf::from("/home/unknown"));
        match agent {
            Agent::Codex => home.join(".codex/sessions"),
            Agent::Claude => home.join(".claude/projects"),
            Agent::Kimi => home.join(".kimi-code/sessions"),
            Agent::Pi => self
                .config
                .pi_agent_dir
                .clone()
                .unwrap_or_else(|| home.join(".pi/agent"))
                .join("sessions"),
            Agent::Grok => self
                .config
                .grok_home
                .clone()
                .unwrap_or_else(|| home.join(".grok"))
                .join("sessions"),
        }
    }
}
fn safe_id(value: &str) -> bool {
    !value.is_empty() && value != "." && value != ".." && !value.contains(['/', '\\', '\0'])
}
fn matches_agent(agent: Agent, path: &Path, id: &str) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    match agent {
        Agent::Codex => name.ends_with(".jsonl") && name.contains(id),
        Agent::Claude => name == format!("{id}.jsonl"),
        Agent::Kimi => path
            .to_string_lossy()
            .contains(&format!("/{id}/agents/main/wire.jsonl")),
        Agent::Pi => name == format!("{id}.jsonl") || name.ends_with(&format!("_{id}.jsonl")),
        Agent::Grok => name == "chat_history.jsonl" && path.to_string_lossy().contains(id),
    }
}
fn walk(
    root: &Path,
    depth_limit: usize,
    file_limit: usize,
    matches: impl Fn(&Path) -> bool,
) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut queue = VecDeque::from([(root.to_owned(), 0usize)]);
    while let Some((directory, depth)) = queue.pop_front() {
        if depth > depth_limit || found.len() >= file_limit {
            break;
        }
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                queue.push_back((path, depth + 1));
            } else if kind.is_file() && matches(&path) {
                found.push(path);
                if found.len() >= file_limit {
                    break;
                }
            }
        }
    }
    found
}
pub(crate) fn file_meta(path: &Path) -> Result<Option<FileMeta>> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_file() {
        return Ok(None);
    }
    let mtime_ms = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis())
        .and_then(|value| u64::try_from(value).ok())
        .unwrap_or(0);
    Ok(Some(FileMeta {
        path: path.to_owned(),
        mtime_ms,
        size: Some(metadata.len()),
        identity: None,
        change_token: None,
        session_id: None,
        created_at_ms: None,
        model_name: None,
        agent_version: None,
    }))
}
