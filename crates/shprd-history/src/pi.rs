use crate::build::Builder;
use crate::json::{arguments, clean, object, output, string, text, timestamp, usage};
use serde_json::{Value, json};

pub(crate) fn project(builder: &mut Builder, records: &[Value], fallback: &str) {
    for (index, record) in records.iter().enumerate() {
        let Some(r) = object(record) else { continue };
        let kind = string(r.get("type"));
        if kind == "session" {
            builder.session_id = Some(string(r.get("id")));
            continue;
        }
        if kind == "model_change" {
            let model = string(r.get("modelId"));
            if !model.is_empty() {
                builder.model = Some(model);
            }
            continue;
        }
        if kind != "message" {
            continue;
        }
        let Some(message) = r.get("message").and_then(object) else {
            continue;
        };
        let at = timestamp(r, fallback);
        let role = string(message.get("role"));
        if role == "user" {
            let body = clean(text(message.get("content")));
            if !body.is_empty() {
                builder.step(at, "user", body);
            }
            continue;
        }
        if role == "toolResult" {
            let body = output(message.get("content"));
            builder.tool_result(
                at,
                Some(string(message.get("toolCallId"))),
                body,
                message
                    .get("isError")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                Some(string(message.get("toolName"))),
            );
            continue;
        }
        if role != "assistant" {
            continue;
        }
        let model = string(message.get("model"));
        if !model.is_empty() {
            builder.model = Some(model);
        }
        let parts = message
            .get("content")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let error = if string(message.get("stopReason")) == "error" {
            clean(string(message.get("errorMessage")))
        } else {
            String::new()
        };
        let first_call = parts
            .iter()
            .position(|p| object(p).is_some_and(|x| string(x.get("type")) == "toolCall"));
        let after_call = first_call.is_some_and(|i| {
            parts[i + 1..].iter().any(|p| {
                object(p).is_some_and(|x| {
                    string(x.get("type")) == "text" && !string(x.get("text")).trim().is_empty()
                })
            })
        });
        if after_call {
            let start = builder.steps.len();
            for part in parts {
                let Some(p) = object(&part) else { continue };
                match string(p.get("type")).as_str() {
                    "toolCall" => builder.tool_call(
                        at.clone(),
                        string(p.get("name")).if_empty("tool".into()),
                        string(p.get("id")).if_empty(format!("{index}:{}", builder.steps.len())),
                        arguments(p.get("arguments")),
                    ),
                    "text" => {
                        let body = clean(string(p.get("text")));
                        if !body.is_empty() {
                            builder.step(at.clone(), "agent", body);
                        }
                    }
                    "thinking" => {
                        let body = clean(string(p.get("thinking")));
                        if !body.is_empty() {
                            let step = builder.step(at.clone(), "agent", "Reasoning".into());
                            step.reasoning_content = Some(body);
                        }
                    }
                    _ => {}
                }
            }
            if !error.is_empty() {
                let step = builder.step(at.clone(), "agent", format!("Error: {error}"));
                step.extra = Some(json!({"error_message":error}));
            }
            if builder.steps.len() > start {
                let metrics = builder.add_usage(usage(message.get("usage")), true);
                if let Some(last) = builder.steps.last_mut() {
                    last.metrics = metrics;
                    last.extra = Some(
                        json!({"record_type":"message","provider":string(message.get("provider")),"model":string(message.get("model")),"stop_reason":string(message.get("stopReason"))}),
                    );
                }
            }
            continue;
        }
        let texts = parts
            .iter()
            .filter_map(object)
            .filter(|p| string(p.get("type")) == "text")
            .map(|p| string(p.get("text")))
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        let reasoning = parts
            .iter()
            .filter_map(object)
            .filter(|p| string(p.get("type")) == "thinking")
            .map(|p| string(p.get("thinking")))
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        let calls = parts
            .iter()
            .filter_map(object)
            .filter(|p| string(p.get("type")) == "toolCall")
            .enumerate()
            .map(|(call, p)| crate::atif::ToolCall {
                tool_call_id: string(p.get("id")).if_empty(format!("{index}:{call}")),
                function_name: string(p.get("name")).if_empty("tool".into()),
                arguments: arguments(p.get("arguments")),
                extra: None,
            })
            .collect::<Vec<_>>();
        if texts.is_empty() && reasoning.is_empty() && calls.is_empty() && error.is_empty() {
            continue;
        }
        let message_text = if !texts.is_empty() {
            clean(texts)
        } else if !calls.is_empty() {
            format!(
                "Tool call{}: {}",
                if calls.len() == 1 { "" } else { "s" },
                calls
                    .iter()
                    .map(|c| c.function_name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        } else if !reasoning.is_empty() {
            "Reasoning".into()
        } else {
            format!("Error: {error}")
        };
        let metrics = builder.add_usage(usage(message.get("usage")), true);
        let step = builder.step(at, "agent", message_text);
        step.reasoning_content = (!reasoning.is_empty()).then_some(clean(reasoning));
        step.tool_calls = (!calls.is_empty()).then_some(calls);
        step.metrics = metrics;
        step.extra = Some(
            json!({"record_type":"message","provider":string(message.get("provider")),"model":string(message.get("model")),"stop_reason":string(message.get("stopReason")),"error_message":if error.is_empty(){Value::Null}else{Value::String(error)}}),
        );
    }
}
trait Empty {
    fn if_empty(self, other: Self) -> Self;
}
impl Empty for String {
    fn if_empty(self, other: Self) -> Self {
        if self.is_empty() { other } else { self }
    }
}
