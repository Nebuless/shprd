use serde_json::{Map, Value};

pub(crate) fn object(value: &Value) -> Option<&Map<String, Value>> {
    value.as_object()
}
pub(crate) fn text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| text(Some(item)))
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Some(Value::Object(item)) => {
            let kind = string(item.get("type"));
            if [
                "tool_result",
                "tool_use",
                "function_call",
                "function_call_output",
                "thinking",
                "think",
            ]
            .contains(&kind.as_str())
            {
                return String::new();
            }
            let direct = string(item.get("text"));
            if !direct.is_empty() {
                return direct;
            }
            text(item.get("content"))
        }
        _ => String::new(),
    }
}
pub(crate) fn string(value: Option<&Value>) -> String {
    value.and_then(Value::as_str).unwrap_or_default().to_owned()
}
pub(crate) fn number(value: Option<&Value>) -> Option<u64> {
    value.and_then(Value::as_u64)
}
pub(crate) fn clean(value: impl AsRef<str>) -> String {
    let mut value = value.as_ref().replace("\r\n", "\n").trim().to_owned();
    for (start, end) in [
        ("<command-message>", "</command-message>"),
        ("<command-name>", "</command-name>"),
        ("<local-command-stdout>", "</local-command-stdout>"),
    ] {
        while let Some(first) = value.find(start) {
            let tail = &value[first + start.len()..];
            let end_index = tail
                .find(end)
                .map(|i| first + start.len() + i + end.len())
                .unwrap_or(value.len());
            value.replace_range(first..end_index, "");
        }
    }
    value = value.trim().to_owned();
    if value == "[Request interrupted by user]" {
        String::new()
    } else {
        value
    }
}
pub(crate) fn arguments(value: Option<&Value>) -> Value {
    match value {
        Some(Value::Object(object)) => Value::Object(object.clone()),
        Some(Value::String(raw)) => {
            serde_json::from_str(raw).unwrap_or_else(|_| serde_json::json!({"value":raw}))
        }
        Some(value) => serde_json::json!({"value":value}),
        None => serde_json::json!({}),
    }
}
pub(crate) fn output(value: Option<&Value>) -> String {
    let text = text(value);
    if !text.is_empty() {
        text
    } else {
        value
            .map(|v| serde_json::to_string_pretty(v).unwrap_or_default())
            .unwrap_or_default()
    }
}
pub(crate) fn timestamp(record: &Map<String, Value>, fallback: &str) -> String {
    for field in ["timestamp", "ts", "time", "created_at", "createdAt"] {
        if let Some(Value::String(value)) = record.get(field)
            && !value.trim().is_empty()
        {
            return value.clone();
        }
    }
    fallback.to_owned()
}
pub(crate) fn usage(value: Option<&Value>) -> Usage {
    let Some(object) = value.and_then(Value::as_object) else {
        return Usage::default();
    };
    let input = number(object.get("input_tokens"))
        .or_else(|| number(object.get("inputTokens")))
        .or_else(|| number(object.get("prompt_tokens")))
        .or_else(|| number(object.get("promptTokens")))
        .or_else(|| number(object.get("inputOther")))
        .or_else(|| number(object.get("input_other")))
        .or_else(|| number(object.get("input")));
    let cache_write = number(object.get("cacheWrite"))
        .or_else(|| number(object.get("inputCacheCreation")))
        .or_else(|| number(object.get("input_cache_creation")));
    let cached = number(object.get("cached_input_tokens"))
        .or_else(|| number(object.get("cache_read_input_tokens")))
        .or_else(|| number(object.get("cacheReadInputTokens")))
        .or_else(|| number(object.get("cacheRead")))
        .or_else(|| number(object.get("inputCacheRead")))
        .or_else(|| number(object.get("input_cache_read")));
    let output = number(object.get("output_tokens"))
        .or_else(|| number(object.get("outputTokens")))
        .or_else(|| number(object.get("completion_tokens")))
        .or_else(|| number(object.get("completionTokens")))
        .or_else(|| number(object.get("output")));
    let reasoning = number(object.get("reasoning_output_tokens"))
        .or_else(|| number(object.get("reasoningOutputTokens")))
        .or_else(|| number(object.get("reasoning")));
    let total = number(object.get("total_tokens"))
        .or_else(|| number(object.get("totalTokens")))
        .or_else(|| number(object.get("tokens")));
    Usage {
        input: input.map(|n| n.saturating_add(cache_write.unwrap_or(0))),
        cached,
        output,
        reasoning,
        total,
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Usage {
    pub input: Option<u64>,
    pub cached: Option<u64>,
    pub output: Option<u64>,
    pub reasoning: Option<u64>,
    pub total: Option<u64>,
}
impl Usage {
    pub(crate) fn any(self) -> bool {
        self.input.is_some()
            || self.cached.is_some()
            || self.output.is_some()
            || self.reasoning.is_some()
            || self.total.is_some()
    }
    pub(crate) fn add(&mut self, other: Self) {
        self.input = add(self.input, other.input);
        self.cached = add(self.cached, other.cached);
        self.output = add(self.output, other.output);
        self.reasoning = add(self.reasoning, other.reasoning);
        self.total = add(self.total, other.total);
    }
}
fn add(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(a), Some(b)) => Some(a.saturating_add(b)),
        (Some(a), None) | (None, Some(a)) => Some(a),
        (None, None) => None,
    }
}
