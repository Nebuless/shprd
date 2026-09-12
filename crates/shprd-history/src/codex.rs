use crate::build::Builder;
use crate::json::{arguments, clean, object, output, string, text, timestamp, usage};
use serde_json::Value;

pub(crate) fn project(builder: &mut Builder, records: &[Value], fallback: &str) {
    let authoritative_user = records.iter().any(|r| {
        object(r)
            .and_then(|x| x.get("type"))
            .and_then(Value::as_str)
            == Some("response_item")
            && object(r)
                .and_then(|x| x.get("payload"))
                .and_then(object)
                .and_then(|p| p.get("type"))
                .and_then(Value::as_str)
                == Some("message")
            && object(r)
                .and_then(|x| x.get("payload"))
                .and_then(object)
                .and_then(|p| p.get("role"))
                .and_then(Value::as_str)
                == Some("user")
    });
    let authoritative_agent = records.iter().any(|r| {
        object(r)
            .and_then(|x| x.get("type"))
            .and_then(Value::as_str)
            == Some("response_item")
            && object(r)
                .and_then(|x| x.get("payload"))
                .and_then(object)
                .and_then(|p| p.get("type"))
                .and_then(Value::as_str)
                == Some("message")
            && object(r)
                .and_then(|x| x.get("payload"))
                .and_then(object)
                .and_then(|p| p.get("role"))
                .and_then(Value::as_str)
                == Some("assistant")
    });
    for (index, record) in records.iter().enumerate() {
        let Some(r) = object(record) else { continue };
        let at = timestamp(r, fallback);
        if string(r.get("type")) == "event_msg" {
            if let Some(payload) = r.get("payload").and_then(object) {
                match string(payload.get("type")).as_str() {
                    "user_message" if !authoritative_user => {
                        let body = clean(string(payload.get("message")));
                        if !body.is_empty() {
                            builder.step(at, "user", body);
                        }
                    }
                    "agent_message" if !authoritative_agent => {
                        let body = clean(string(payload.get("message")));
                        if !body.is_empty() {
                            builder.step(at, "agent", body);
                        }
                    }
                    "token_count" => {
                        if let Some(info) = payload.get("info").and_then(object) {
                            let metrics =
                                builder.add_usage(usage(info.get("total_token_usage")), false);
                            if let Some(metrics) = metrics {
                                builder.step(at, "system", "Token usage".into()).metrics =
                                    Some(metrics);
                            }
                        }
                    }
                    _ => {}
                }
            }
            continue;
        }
        if string(r.get("type")) != "response_item" {
            continue;
        }
        let Some(p) = r.get("payload").and_then(object) else {
            continue;
        };
        let kind = string(p.get("type"));
        if kind == "message" {
            let body = clean(text(p.get("content")));
            if !body.is_empty() {
                let source = if string(p.get("role")) == "user" {
                    "user"
                } else if string(p.get("role")) == "assistant" {
                    "agent"
                } else {
                    "system"
                };
                let metrics = builder.add_usage(usage(p.get("usage")), false);
                builder.step(at, source, body).metrics = metrics;
            }
        } else if kind == "reasoning" {
            let body = clean(text(p.get("summary")));
            let step = builder.step(
                at,
                "agent",
                if body.is_empty() {
                    "Reasoning".into()
                } else {
                    body.clone()
                },
            );
            step.reasoning_content = (!body.is_empty()).then_some(body);
        } else if kind.contains("output") || kind.contains("result") || kind.contains("tool") {
            let body = output(p.get("output").or_else(|| p.get("content")));
            builder.tool_result(
                at,
                Some(string(p.get("call_id")).if_empty(string(p.get("id")))),
                body,
                string(p.get("status")) == "failed" || string(p.get("status")) == "error",
                None,
            );
        } else if kind.contains("tool_call") || kind.contains("function_call") {
            builder.tool_call(
                at,
                string(p.get("name"))
                    .if_empty(string(p.get("call_name")))
                    .if_empty(kind.clone()),
                string(p.get("call_id"))
                    .if_empty(string(p.get("id")))
                    .if_empty(index.to_string()),
                arguments(p.get("arguments").or_else(|| p.get("input"))),
            );
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
