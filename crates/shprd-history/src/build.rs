use crate::atif::{
    AgentInfo, Atif, FinalMetrics, Metrics, Observation, ObservationWrap, Step, ToolCall,
};
use crate::json::Usage;
use crate::{Agent, FileMeta};
use serde_json::{Value, json};

pub(crate) struct Builder {
    pub agent: Agent,
    pub file: FileMeta,
    pub records: usize,
    pub session_id: Option<String>,
    pub model: Option<String>,
    pub version: Option<String>,
    pub steps: Vec<Step>,
    pub usage: Usage,
}
impl Builder {
    pub(crate) fn new(agent: Agent, file: FileMeta, records: usize) -> Self {
        Self {
            agent,
            file,
            records,
            session_id: None,
            model: None,
            version: None,
            steps: Vec::new(),
            usage: Usage::default(),
        }
    }
    pub(crate) fn step(&mut self, timestamp: String, source: &str, message: String) -> &mut Step {
        let id = self.steps.len() + 1;
        self.steps.push(Step {
            step_id: id,
            timestamp: Some(timestamp),
            source: source.into(),
            message,
            reasoning_content: None,
            tool_calls: None,
            observation: None,
            metrics: None,
            extra: None,
        });
        self.steps.last_mut().expect("pushed")
    }
    pub(crate) fn add_usage(&mut self, usage: Usage, pi_includes_cache: bool) -> Option<Metrics> {
        self.usage.add(usage);
        if !usage.any() {
            return None;
        }
        let prompt = usage.input.map(|v| {
            if pi_includes_cache {
                v.saturating_add(usage.cached.unwrap_or(0))
            } else {
                v
            }
        });
        Some(Metrics {
            prompt_tokens: prompt,
            completion_tokens: usage.output,
            cached_tokens: usage.cached,
            extra: match (usage.reasoning, usage.total) {
                (None, None) => None,
                (reason, total) => {
                    Some(json!({"reasoning_output_tokens":reason,"total_tokens":total}))
                }
            },
        })
    }
    pub(crate) fn tool_call(
        &mut self,
        timestamp: String,
        name: String,
        id: String,
        arguments: Value,
    ) {
        let step = self.step(timestamp, "agent", format!("Tool call: {name}"));
        step.tool_calls = Some(vec![ToolCall {
            tool_call_id: id,
            function_name: name,
            arguments,
            extra: None,
        }]);
    }
    pub(crate) fn tool_result(
        &mut self,
        timestamp: String,
        id: Option<String>,
        content: String,
        error: bool,
        name: Option<String>,
    ) {
        let step = self.step(
            timestamp,
            "system",
            if content.is_empty() {
                if error {
                    "Tool failed".into()
                } else {
                    "Tool result".into()
                }
            } else {
                content.clone()
            },
        );
        step.observation = Some(ObservationWrap {
            results: vec![Observation {
                source_call_id: id,
                content: Some(content),
                extra: Some(json!({"is_error":error,"tool_name":name})),
            }],
        });
    }
    pub(crate) fn finish(mut self) -> Atif {
        self.steps.retain(|s| {
            !s.message.trim().is_empty()
                || s.reasoning_content
                    .as_ref()
                    .is_some_and(|x| !x.trim().is_empty())
                || s.tool_calls.as_ref().is_some_and(|x| !x.is_empty())
                || s.observation
                    .as_ref()
                    .is_some_and(|x| !x.results.is_empty())
        });
        for (index, step) in self.steps.iter_mut().enumerate() {
            step.step_id = index + 1;
        }
        let total = if self.usage.any() {
            Some(FinalMetrics {
                total_prompt_tokens: self.usage.input.map(|v| {
                    if self.agent == Agent::Pi {
                        v.saturating_add(self.usage.cached.unwrap_or(0))
                    } else {
                        v
                    }
                }),
                total_completion_tokens: self.usage.output,
                total_cached_tokens: self.usage.cached,
                total_steps: self.steps.len(),
                extra: match (self.usage.reasoning, self.usage.total) {
                    (None, None) => None,
                    (reason, total) => {
                        Some(json!({"reasoning_output_tokens":reason,"total_tokens":total}))
                    }
                },
            })
        } else {
            Some(FinalMetrics {
                total_prompt_tokens: None,
                total_completion_tokens: None,
                total_cached_tokens: None,
                total_steps: self.steps.len(),
                extra: None,
            })
        };
        let session = self
            .session_id
            .clone()
            .or_else(|| self.file.session_id.clone())
            .or_else(|| {
                self.file
                    .path
                    .file_stem()
                    .map(|x| x.to_string_lossy().to_string())
            });
        Atif {
            schema_version: "ATIF-v1.7",
            session_id: session.clone(),
            trajectory_id: session,
            agent: AgentInfo {
                name: self.agent.atif_name().into(),
                version: self
                    .file
                    .agent_version
                    .clone()
                    .or(self.version)
                    .unwrap_or_else(|| {
                        if self.agent == Agent::Kimi {
                            "kimi-code".into()
                        } else {
                            "unknown".into()
                        }
                    }),
                model_name: self.file.model_name.clone().or(self.model),
            },
            steps: self.steps,
            final_metrics: total,
            extra: Some(
                json!({"source_path":self.file.path,"source_records":self.records,"projection":"shprd-gui-lightweight"}),
            ),
        }
    }
}
