use crate::Projection;
use serde::Serialize;

pub const MAX_PREVIEW_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TokenUsage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_tokens: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SessionStats {
    pub turns: usize,
    pub records: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_usage: Option<TokenUsage>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SessionView {
    pub stats: SessionStats,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trajectory: Option<serde_json::Value>,
}

pub fn summarize(
    projection: &Projection,
    text: Option<&str>,
    include_trajectory: bool,
) -> crate::Result<SessionView> {
    let limit = MAX_PREVIEW_BYTES;
    let (text, truncated) = match text {
        Some(text) => {
            let end = text.len().min(limit);
            let end = text.floor_char_boundary(end);
            (Some(text[..end].to_owned()), text.len() > end)
        }
        None => (None, false),
    };
    let metrics = projection.atif.final_metrics.as_ref();
    let token_usage = metrics.and_then(|metrics| {
        (metrics.total_prompt_tokens.is_some()
            || metrics.total_completion_tokens.is_some()
            || metrics.total_cached_tokens.is_some())
        .then_some(TokenUsage {
            prompt_tokens: metrics.total_prompt_tokens,
            completion_tokens: metrics.total_completion_tokens,
            cached_tokens: metrics.total_cached_tokens,
        })
    });
    let turns = projection
        .v1_messages
        .iter()
        .filter(|message| message.role == "user")
        .count();
    let trajectory = include_trajectory
        .then(|| serde_json::to_value(&projection.atif))
        .transpose()?;
    Ok(SessionView {
        stats: SessionStats {
            turns,
            records: projection
                .atif
                .extra
                .as_ref()
                .and_then(|extra| extra.get("source_records"))
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or(0),
            token_usage,
        },
        text,
        truncated,
        trajectory,
    })
}
