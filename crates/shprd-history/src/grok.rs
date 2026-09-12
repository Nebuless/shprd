use crate::build::Builder;
use crate::json::{arguments, clean, object, output, string, text, timestamp, usage};
use serde_json::Value;

pub(crate) fn project(builder: &mut Builder, records: &[Value], fallback: &str) {
    for (index, record) in records.iter().enumerate() {
        let Some(record) = object(record) else {
            continue;
        };
        let at = timestamp(record, fallback);
        match string(record.get("type")).as_str() {
            "user" if !record.contains_key("synthetic_reason") => {
                let content = text(record.get("content"));
                let body = clean(user_query(&content).unwrap_or_else(|| strip_context(&content)));
                if !body.is_empty() {
                    builder.step(at, "user", body);
                }
            }
            "system" => {
                let body = clean(text(record.get("content")));
                if !body.is_empty() {
                    builder.step(at, "system", body);
                }
            }
            "reasoning" => {
                let body = clean(text(
                    record.get("summary").or_else(|| record.get("content")),
                ));
                if !body.is_empty() {
                    let step = builder.step(at, "agent", "Reasoning".into());
                    step.reasoning_content = Some(body);
                }
            }
            "assistant" => project_assistant(builder, record, at, index),
            "tool_result" => builder.tool_result(
                at,
                Some(string(record.get("tool_call_id"))),
                output(record.get("content")),
                matches!(string(record.get("status")).as_str(), "error" | "failed"),
                None,
            ),
            _ => (),
        }
    }
}

fn project_assistant(
    builder: &mut Builder,
    record: &serde_json::Map<String, Value>,
    at: String,
    index: usize,
) {
    let body = clean(text(record.get("content")));
    let calls = record
        .get("tool_calls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(object)
        .enumerate()
        .map(|(call, item)| crate::atif::ToolCall {
            tool_call_id: non_empty(string(item.get("id")), format!("{index}:{call}")),
            function_name: non_empty(string(item.get("name")), "tool".into()),
            arguments: arguments(item.get("arguments")),
            extra: None,
        })
        .collect::<Vec<_>>();
    if body.is_empty() && calls.is_empty() {
        return;
    }
    let metrics = builder.add_usage(usage(record.get("usage")), false);
    let step = builder.step(
        at,
        "agent",
        if body.is_empty() {
            format!(
                "Tool call{}: {}",
                if calls.len() == 1 { "" } else { "s" },
                calls
                    .iter()
                    .map(|call| call.function_name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        } else {
            body
        },
    );
    step.tool_calls = (!calls.is_empty()).then_some(calls);
    step.metrics = metrics;
}

fn user_query(value: &str) -> Option<String> {
    let start = value.find("<user_query>")? + "<user_query>".len();
    let end = value[start..].find("</user_query>")? + start;
    Some(value[start..end].to_owned())
}
fn strip_context(value: &str) -> String {
    let mut out = value.to_owned();
    for (start, end) in [
        ("<user_info>", "</user_info>"),
        ("<git_status>", "</git_status>"),
        ("<system-reminder>", "</system-reminder>"),
    ] {
        while let Some(from) = out.find(start) {
            let to = out[from + start.len()..]
                .find(end)
                .map(|i| from + start.len() + i + end.len())
                .unwrap_or(out.len());
            out.replace_range(from..to, "");
        }
    }
    out
}
fn non_empty(value: String, fallback: String) -> String {
    if value.is_empty() { fallback } else { value }
}
