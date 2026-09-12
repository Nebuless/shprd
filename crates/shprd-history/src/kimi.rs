use crate::build::Builder;
use crate::json::{arguments, clean, object, output, string, timestamp, usage};
use serde_json::Value;
use std::collections::BTreeMap;

pub(crate) fn project(builder: &mut Builder, records: &[Value], fallback: &str) {
    let mut by_step = BTreeMap::<String, usize>::new();
    for (index, record) in records.iter().enumerate() {
        let Some(r) = object(record) else { continue };
        let kind = string(r.get("type"));
        let at = timestamp(r, fallback);
        if kind == "context.append_message" {
            if let Some(message) = r.get("message").and_then(object)
                && string(message.get("role")) == "user"
            {
                let body = clean(crate::json::text(message.get("content")));
                if !body.is_empty() {
                    builder.step(at, "user", body);
                }
            }
            continue;
        }
        if kind != "context.append_loop_event" {
            continue;
        }
        let Some(event) = r.get("event").and_then(object) else {
            continue;
        };
        let key = format!(
            "{}:{}",
            string(event.get("turnId")),
            string(event.get("step"))
        );
        match string(event.get("type")).as_str() {
            "content.part" => {
                if let Some(part) = event.get("part").and_then(object) {
                    match string(part.get("type")).as_str() {
                        "text" => {
                            let body = clean(string(part.get("text")));
                            if !body.is_empty() {
                                builder.step(at, "agent", body);
                                by_step.insert(key, builder.steps.len() - 1);
                            }
                        }
                        "think" => {
                            let body = clean(string(part.get("think")));
                            if !body.is_empty() {
                                let step = builder.step(at, "agent", "Reasoning".into());
                                step.reasoning_content = Some(body);
                                by_step.insert(key, builder.steps.len() - 1);
                            }
                        }
                        _ => {}
                    }
                }
            }
            "tool.call" => {
                builder.tool_call(
                    at,
                    string(event.get("name")).if_empty("tool".into()),
                    string(event.get("toolCallId")).if_empty(index.to_string()),
                    arguments(event.get("args")),
                );
            }
            "tool.result" => {
                let result = event.get("result").and_then(object);
                builder.tool_result(
                    at,
                    Some(string(event.get("toolCallId"))),
                    output(result.and_then(|x| x.get("output"))),
                    result
                        .and_then(|x| x.get("isError"))
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    None,
                );
            }
            "step.end" => {
                if let Some(found) = by_step.get(&key).copied() {
                    let metrics = builder.add_usage(usage(event.get("usage")), false);
                    if let Some(step) = builder.steps.get_mut(found) {
                        step.metrics = metrics;
                    }
                }
            }
            _ => {}
        }
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
