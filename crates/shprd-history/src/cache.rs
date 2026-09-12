use crate::{Cursor, FileMeta, PaneMetadata, Projection, Result, Update, history, project};
use std::collections::{BTreeMap, HashMap, VecDeque};

#[derive(Clone, Debug)]
struct Cached {
    signature: String,
    epoch: String,
    revision: u64,
    projection: Projection,
    versions: BTreeMap<u64, Vec<history::Entry>>,
    bytes: usize,
}

#[derive(Clone, Debug)]
pub struct Cache {
    entries: usize,
    bytes: usize,
    retained: usize,
    epoch_counter: u64,
    items: HashMap<String, Cached>,
    order: VecDeque<String>,
}

impl Cache {
    pub fn new(entries: usize, bytes: usize) -> Self {
        Self {
            entries,
            bytes,
            retained: 0,
            epoch_counter: 0,
            items: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    pub fn history(
        &mut self,
        pane: &PaneMetadata,
        file: FileMeta,
        text: &str,
        cursor: Option<&Cursor>,
    ) -> Result<(Projection, Update)> {
        let key = Self::key(pane, &file);
        let signature = Self::signature(&file);
        if let Some(old) = self
            .items
            .get(&key)
            .filter(|old| old.signature == signature)
            .cloned()
        {
            self.touch(&key);
            let update = history::update(
                &old.epoch,
                old.revision,
                &old.projection.entries,
                cursor,
                &old.versions,
            );
            return Ok((old.projection, update));
        }

        let projection = project::project(pane, file.clone(), text)?;
        let old = self.items.get(&key).cloned();
        let reset = old.as_ref().is_none_or(|old| {
            old.projection.file.path != file.path
                || old.projection.file.identity != file.identity
                || file.size.unwrap_or(0) < old.projection.file.size.unwrap_or(0)
        });
        let revision = if reset {
            1
        } else {
            old.as_ref()
                .map(|old| old.revision.saturating_add(1))
                .unwrap_or(1)
        };
        let mut versions = if reset {
            BTreeMap::new()
        } else {
            old.as_ref()
                .map(|old| old.versions.clone())
                .unwrap_or_default()
        };
        versions.insert(revision, projection.entries.clone());
        while versions.len() > 4 {
            if let Some(first) = versions.keys().next().copied() {
                versions.remove(&first);
            }
        }
        self.epoch_counter = self.epoch_counter.saturating_add(1);
        let epoch = if reset {
            format!("history-{}", self.epoch_counter)
        } else {
            old.as_ref()
                .map(|old| old.epoch.clone())
                .unwrap_or_else(|| format!("history-{}", self.epoch_counter))
        };
        let bytes = Self::estimate(&projection, &versions, self.bytes);
        if let Some(old) = self.items.remove(&key) {
            self.retained = self.retained.saturating_sub(old.bytes);
            self.order.retain(|item| item != &key);
        }
        let update = history::update(&epoch, revision, &projection.entries, cursor, &versions);
        if bytes <= self.bytes && self.entries > 0 {
            self.retained = self.retained.saturating_add(bytes);
            self.items.insert(
                key.clone(),
                Cached {
                    signature,
                    epoch,
                    revision,
                    projection: projection.clone(),
                    versions,
                    bytes,
                },
            );
            self.touch(&key);
            self.evict();
        }
        Ok((projection, update))
    }

    fn key(pane: &PaneMetadata, file: &FileMeta) -> String {
        format!("{}:{}", pane.agent.canonical_name(), file.path.display())
    }

    fn signature(file: &FileMeta) -> String {
        format!(
            "{}|{}|{:?}|{:?}|{:?}|{:?}",
            file.path.display(),
            file.mtime_ms,
            file.size,
            file.identity,
            file.change_token,
            file.created_at_ms,
        )
    }

    fn estimate(
        projection: &Projection,
        versions: &BTreeMap<u64, Vec<history::Entry>>,
        budget: usize,
    ) -> usize {
        let mut used: usize = 128;
        for text in projection
            .atif
            .steps
            .iter()
            .map(|step| step.message.as_str())
            .chain(projection.entries.iter().map(|entry| entry.text.as_str()))
            .chain(versions.values().flatten().map(|entry| entry.text.as_str()))
        {
            used = used.saturating_add(32 + text.len().saturating_mul(2));
            if used > budget {
                return usize::MAX;
            }
        }
        used
    }

    fn touch(&mut self, key: &str) {
        self.order.retain(|item| item != key);
        self.order.push_back(key.to_owned());
    }

    fn evict(&mut self) {
        while self.items.len() > self.entries || self.retained > self.bytes {
            let Some(key) = self.order.pop_front() else {
                break;
            };
            if let Some(old) = self.items.remove(&key) {
                self.retained = self.retained.saturating_sub(old.bytes);
            }
        }
    }
}
