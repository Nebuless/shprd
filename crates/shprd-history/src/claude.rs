use crate::build::Builder;
use crate::json::{arguments, clean, object, output, string, text, timestamp, usage};
use serde_json::Value;

pub(crate) fn project(builder: &mut Builder, records: &[Value], fallback: &str) {
    for (index, record) in records.iter().enumerate() {
        let Some(record) = object(record) else {
            continue;
        };
        let kind = string(record.get("type"));
        if !matches!(kind.as_str(), "user" | "assistant" | "system") {
            continue;
        }
        let message = record.get("message").and_then(object).unwrap_or(record);
        let role = non_empty(string(message.get("role")), kind);
        let at = timestamp(record, fallback);
        let parts = message
            .get("content")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if parts.iter().any(is_tool_part) {
            project_parts(builder, &parts, &at, &role, index, message.get("usage"));
            continue;
        }
        let body = clean(text(
            message.get("content").or_else(|| record.get("content")),
        ));
        let record_usage = usage(record.get("usage").or_else(|| message.get("usage")));
        if body.is_empty() && !record_usage.any() {
            continue;
        }
        let metrics = builder.add_usage(record_usage, false);
        builder
            .step(
                at,
                source(&role),
                if body.is_empty() {
                    "Token usage".into()
                } else {
                    body
                },
            )
            .metrics = metrics;
    }
}

fn project_parts(
    builder: &mut Builder,
    parts: &[Value],
    at: &str,
    role: &str,
    index: usize,
    record_usage: Option<&Value>,
) {
    let start = builder.steps.len();
    for part in parts {
        let Some(part) = object(part) else { continue };
        match string(part.get("type")).as_str() {
            "text" => {
                let body = clean(string(part.get("text")));
                if !body.is_empty() {
                    builder.step(at.into(), source(role), body);
                }
            }
            "thinking" => {
                let body = clean(string(part.get("thinking")));
                if !body.is_empty() {
                    let step = builder.step(at.into(), "agent", "Reasoning".into());
                    step.reasoning_content = Some(body);
                }
            }
            "tool_use" => builder.tool_call(
                at.into(),
                non_empty(string(part.get("name")), "tool".into()),
                non_empty(
                    string(part.get("id")),
                    format!("{index}:{}", builder.steps.len()),
                ),
                arguments(part.get("input")),
            ),
            "tool_result" => builder.tool_result(
                at.into(),
                Some(string(part.get("tool_use_id"))),
                output(part.get("content")),
                part.get("is_error")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                None,
            ),
            _ => (),
        }
    }
    if builder.steps.len() > start {
        let metrics = builder.add_usage(usage(record_usage), false);
        if let Some(last) = builder.steps.last_mut() {
            last.metrics = metrics;
        }
    }
}

fn is_tool_part(part: &Value) -> bool {
    object(part).is_some_and(|part| {
        matches!(
            string(part.get("type")).as_str(),
            "tool_use" | "tool_result"
        )
    })
}
fn source(role: &str) -> &'static str {
    match role {
        "user" => "user",
        "assistant" => "agent",
        _ => "system",
    }
}
fn non_empty(value: String, fallback: String) -> String {
    if value.is_empty() { fallback } else { value }
}
