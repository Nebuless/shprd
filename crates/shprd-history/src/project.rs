use crate::build::Builder;
use crate::history::{Projection, entries, messages};
use crate::{Agent, FileMeta, PaneMetadata, Result, error::invalid};
use serde_json::Value;

pub(crate) fn records(text: &str) -> Vec<Value> {
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
        .filter(Value::is_object)
        .collect()
}

pub(crate) fn project(pane: &PaneMetadata, file: FileMeta, text: &str) -> Result<Projection> {
    pane.validated()?;
    if pane.agent != Agent::Grok && !file.path.is_absolute() {
        return Err(invalid("session transcript path must be absolute"));
    }
    let rows = records(text);
    let fallback = file.mtime_ms.to_string();
    let mut builder = Builder::new(pane.agent, file.clone(), rows.len());
    match pane.agent {
        Agent::Codex => crate::codex::project(&mut builder, &rows, &fallback),
        Agent::Claude => crate::claude::project(&mut builder, &rows, &fallback),
        Agent::Kimi => crate::kimi::project(&mut builder, &rows, &fallback),
        Agent::Grok => crate::grok::project(&mut builder, &rows, &fallback),
        Agent::Pi => crate::pi::project(&mut builder, &rows, &fallback),
    }
    let atif = builder.finish();
    let v1_messages = messages(file.path.to_str().unwrap_or("session.jsonl"), &atif);
    let entries = entries(&atif);
    Ok(Projection {
        file,
        atif,
        v1_messages,
        entries,
    })
}
