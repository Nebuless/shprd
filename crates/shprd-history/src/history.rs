use crate::atif::Atif;
use crate::{Error, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};

pub const WINDOW_LIMIT: usize = 200;
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Message {
    pub id: String,
    pub role: String,
    pub text: String,
    pub sent_at: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub id: String,
    pub role: String,
    pub kind: String,
    pub text: String,
    pub sent_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_bytes: Option<usize>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Cursor {
    pub epoch: String,
    pub revision: u64,
}
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum Update {
    Snapshot {
        history_version: u8,
        cursor: Cursor,
        window_limit: usize,
        entries: Vec<Entry>,
    },
    Delta {
        history_version: u8,
        cursor: Cursor,
        window_limit: usize,
        base_revision: u64,
        upserts: Vec<Entry>,
        removed: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        order: Option<Vec<String>>,
    },
}
#[derive(Clone, Debug)]
pub struct Projection {
    pub file: crate::FileMeta,
    pub atif: Atif,
    pub v1_messages: Vec<Message>,
    pub entries: Vec<Entry>,
}
impl Projection {
    pub fn entry(&self, id: &str) -> Result<&Entry> {
        self.entries
            .iter()
            .find(|entry| entry.id == id)
            .ok_or(Error::MissingEntry)
    }
}
pub(crate) fn messages(file: &str, atif: &Atif) -> Vec<Message> {
    atif.steps
        .iter()
        .filter(|step| conversation(step))
        .map(|step| Message {
            id: format!(
                "{}:{}",
                file.rsplit('/').next().unwrap_or("session.jsonl"),
                step.step_id
            ),
            role: if step.source == "user" {
                "user".into()
            } else {
                "assistant".into()
            },
            text: step.message.clone(),
            sent_at: step.timestamp.clone().unwrap_or_default(),
        })
        .collect()
}
fn conversation(step: &crate::atif::Step) -> bool {
    if step.source == "user" {
        return !step.message.trim().is_empty();
    }
    if step.source != "agent" || step.message.trim().is_empty() {
        return false;
    }
    !(step.metrics.is_some() && step.message == "Token usage")
        && !(step.reasoning_content.is_some() && step.message == "Reasoning")
        && !(step.tool_calls.is_some() && step.message.starts_with("Tool call"))
        && step
            .extra
            .as_ref()
            .and_then(|v| v.get("error_message"))
            .is_none()
}
pub(crate) fn entries(atif: &Atif) -> Vec<Entry> {
    let mut output = Vec::new();
    let mut seen = std::collections::BTreeMap::<String, usize>::new();
    let mut tool_names = std::collections::BTreeMap::new();
    for step in &atif.steps {
        if let Some(calls) = &step.tool_calls {
            for call in calls {
                tool_names.insert(call.tool_call_id.clone(), call.function_name.clone());
            }
        }
    }
    let mut push = |role: &str,
                    kind: &str,
                    text: String,
                    time: String,
                    tool_name: Option<String>,
                    source_call_id: Option<String>,
                    is_error: Option<bool>| {
        let identity = if let Some(call) = &source_call_id {
            format!("{kind}\0{call}")
        } else {
            format!("{kind}\0{role}\0{text}")
        };
        let mut digest = Sha256::new();
        digest.update(identity);
        let hash = hex::encode(digest.finalize());
        let occurrence = seen.entry(hash.clone()).or_default();
        let id = format!("{}:{}", &hash[..24], *occurrence);
        *occurrence += 1;
        output.push(Entry {
            id,
            role: role.into(),
            kind: kind.into(),
            text,
            sent_at: time,
            tool_name,
            source_call_id,
            is_error,
            text_bytes: None,
        });
    };
    for step in &atif.steps {
        let time = step.timestamp.clone().unwrap_or_default();
        if step.observation.is_none() && conversation(step) {
            push(
                if step.source == "user" {
                    "user"
                } else {
                    "assistant"
                },
                "message",
                step.message.clone(),
                time.clone(),
                None,
                None,
                None,
            );
        }
        if let Some(calls) = &step.tool_calls {
            for call in calls {
                push(
                    "tool",
                    "tool_call",
                    serde_json::to_string_pretty(&call.arguments).unwrap_or_default(),
                    time.clone(),
                    Some(call.function_name.clone()),
                    Some(call.tool_call_id.clone()),
                    None,
                );
            }
        }
        if let Some(observation) = &step.observation {
            for result in &observation.results {
                let name = result
                    .extra
                    .as_ref()
                    .and_then(|x| x.get("tool_name"))
                    .and_then(|x| x.as_str())
                    .map(str::to_owned)
                    .or_else(|| {
                        result
                            .source_call_id
                            .as_ref()
                            .and_then(|id| tool_names.get(id).cloned())
                    });
                let error = result
                    .extra
                    .as_ref()
                    .and_then(|x| x.get("is_error"))
                    .and_then(|x| x.as_bool());
                push(
                    "tool",
                    "tool_result",
                    result
                        .content
                        .clone()
                        .unwrap_or_else(|| step.message.clone()),
                    time.clone(),
                    name,
                    result.source_call_id.clone(),
                    error,
                );
            }
        }
        if let Some(error) = step
            .extra
            .as_ref()
            .and_then(|x| x.get("error_message"))
            .and_then(|x| x.as_str())
        {
            push(
                "assistant",
                "error",
                error.into(),
                time,
                None,
                None,
                Some(true),
            );
        }
    }
    window(output)
}
fn window(entries: Vec<Entry>) -> Vec<Entry> {
    let mut remaining = WINDOW_LIMIT;
    let mut start = entries.len();
    while start > 0 && remaining > 0 {
        start -= 1;
        if entries[start].role != "tool" {
            remaining -= 1;
        }
    }
    entries.into_iter().skip(start).collect()
}
pub fn redacted(entries: &[Entry]) -> Vec<Entry> {
    entries
        .iter()
        .cloned()
        .map(|mut entry| {
            if entry.role == "tool" && !entry.text.is_empty() {
                entry.text_bytes = Some(entry.text.len());
                entry.text.clear();
            }
            entry
        })
        .collect()
}
pub fn update(
    epoch: &str,
    revision: u64,
    current: &[Entry],
    cursor: Option<&Cursor>,
    versions: &std::collections::BTreeMap<u64, Vec<Entry>>,
) -> Update {
    let snapshot = || Update::Snapshot {
        history_version: 2,
        cursor: Cursor {
            epoch: epoch.into(),
            revision,
        },
        window_limit: WINDOW_LIMIT,
        entries: redacted(current),
    };
    let Some(previous_cursor) = cursor else {
        return snapshot();
    };
    if previous_cursor.epoch != epoch || previous_cursor.revision > revision {
        return snapshot();
    }
    let Some(previous) = versions.get(&previous_cursor.revision) else {
        return snapshot();
    };
    if previous_cursor.revision == revision {
        return Update::Delta {
            history_version: 2,
            cursor: Cursor {
                epoch: epoch.into(),
                revision,
            },
            window_limit: WINDOW_LIMIT,
            base_revision: revision,
            upserts: vec![],
            removed: vec![],
            order: None,
        };
    }
    let old: std::collections::BTreeMap<_, _> =
        previous.iter().map(|e| (e.id.clone(), e)).collect();
    let ids: std::collections::BTreeSet<_> = current.iter().map(|e| e.id.clone()).collect();
    let upserts = current
        .iter()
        .filter(|entry| old.get(&entry.id).is_none_or(|old| *old != *entry))
        .cloned()
        .collect::<Vec<_>>();
    let old_order = previous.iter().map(|e| &e.id).collect::<Vec<_>>();
    let order = current.iter().map(|e| e.id.clone()).collect::<Vec<_>>();
    Update::Delta {
        history_version: 2,
        cursor: Cursor {
            epoch: epoch.into(),
            revision,
        },
        window_limit: WINDOW_LIMIT,
        base_revision: previous_cursor.revision,
        upserts: redacted(&upserts),
        removed: previous
            .iter()
            .filter(|e| !ids.contains(&e.id))
            .map(|e| e.id.clone())
            .collect(),
        order: if old_order
            .iter()
            .map(|x| x.as_str())
            .eq(order.iter().map(String::as_str))
        {
            None
        } else {
            Some(order)
        },
    }
}
