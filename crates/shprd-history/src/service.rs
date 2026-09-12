use crate::cache::Cache;
use crate::{
    Agent, Cursor, Entry, FileMeta, HostConfig, PaneMetadata, Projection, Result, Update, history,
    project,
};
use serde_json::Value;
use std::sync::Mutex;

/// Readonly local projection service. Host supplies already-resolved pane metadata.
pub struct HistoryService {
    config: HostConfig,
    cache: Mutex<Cache>,
}

impl HistoryService {
    pub fn new(config: HostConfig) -> Self {
        let config = config.bounded();
        Self {
            cache: Mutex::new(Cache::new(config.cache_entries, config.cache_bytes)),
            config,
        }
    }

    pub fn supported_agents(&self) -> &[Agent] {
        &Agent::ALL
    }

    pub fn project_text(
        &self,
        pane: &PaneMetadata,
        file: FileMeta,
        text: &str,
    ) -> Result<Projection> {
        project::project(pane, file, text)
    }

    pub fn history_snapshot(&self, projection: &Projection) -> Update {
        Update::Snapshot {
            history_version: 2,
            cursor: Cursor {
                epoch: "standalone".into(),
                revision: 1,
            },
            window_limit: history::WINDOW_LIMIT,
            entries: history::redacted(&projection.entries),
        }
    }

    pub fn history(
        &self,
        pane: &PaneMetadata,
        file: FileMeta,
        text: &str,
        cursor: Option<&Cursor>,
    ) -> Result<(Projection, Update)> {
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| crate::Error::Invalid("history cache lock poisoned".into()))?;
        cache.history(pane, file, text, cursor)
    }

    pub fn entry<'a>(&self, projection: &'a Projection, id: &str) -> Result<&'a Entry> {
        projection.entry(id)
    }

    pub fn atif_json(&self, projection: &Projection) -> Result<Value> {
        Ok(serde_json::to_value(&projection.atif)?)
    }

    pub fn session_view(
        &self,
        projection: &Projection,
        text: Option<&str>,
        include_trajectory: bool,
    ) -> Result<crate::SessionView> {
        crate::summary::summarize(projection, text, include_trajectory)
    }

    pub const fn config(&self) -> &HostConfig {
        &self.config
    }
}
