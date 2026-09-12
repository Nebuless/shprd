use serde_json::{Value, json};
use shprd_history::{Agent, FileMeta, HistoryService, HostConfig, PaneMetadata};

fn project(agent: Agent, records: Vec<Value>) -> shprd_history::Projection {
    let text = records
        .into_iter()
        .map(|record| record.to_string())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let path = format!("/fixture/{}.jsonl", agent.canonical_name());
    HistoryService::new(HostConfig::default())
        .project_text(
            &PaneMetadata::new("pane", "workspace", "tab", agent, &path),
            FileMeta::fixture(path, &text, 1),
            &text,
        )
        .unwrap()
}

#[test]
fn supported_agent_fixtures_project_user_agent_tool_and_atif() {
    let cases = [
        (
            Agent::Codex,
            vec![
                json!({"type":"response_item","payload":{"type":"message","role":"user","content":"codex user"}}),
                json!({"type":"response_item","payload":{"type":"function_call","id":"c","name":"read","arguments":{"path":"x"}}}),
                json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"c","output":"codex result"}}),
            ],
        ),
        (
            Agent::Claude,
            vec![
                json!({"type":"user","message":{"role":"user","content":"claude user"}}),
                json!({"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"c","name":"read","input":{"path":"x"}},{"type":"tool_result","tool_use_id":"c","content":"claude result"}]}}),
            ],
        ),
        (
            Agent::Kimi,
            vec![
                json!({"type":"context.append_message","message":{"role":"user","content":"kimi user"}}),
                json!({"type":"context.append_loop_event","event":{"type":"tool.call","turnId":"turn","step":1,"toolCallId":"c","name":"read","args":{"path":"x"}}}),
                json!({"type":"context.append_loop_event","event":{"type":"tool.result","turnId":"turn","step":1,"toolCallId":"c","result":{"output":"kimi result"}}}),
            ],
        ),
        (
            Agent::Grok,
            vec![
                json!({"type":"user","content":"grok user"}),
                json!({"type":"assistant","tool_calls":[{"id":"c","name":"read","arguments":{"path":"x"}}]}),
                json!({"type":"tool_result","tool_call_id":"c","content":"grok result"}),
            ],
        ),
        (
            Agent::Pi,
            vec![
                json!({"type":"session","id":"pi-session"}),
                json!({"type":"message","message":{"role":"user","content":"pi user"}}),
                json!({"type":"message","message":{"role":"assistant","content":[{"type":"toolCall","id":"c","name":"read","arguments":{"path":"x"}}]}}),
                json!({"type":"message","message":{"role":"toolResult","toolCallId":"c","toolName":"read","content":"pi result"}}),
            ],
        ),
    ];
    for (agent, records) in cases {
        let projection = project(agent, records);
        assert_eq!(projection.atif.schema_version, "ATIF-v1.7", "{agent:?}");
        assert_eq!(projection.atif.agent.name, agent.atif_name(), "{agent:?}");
        assert!(
            projection
                .v1_messages
                .iter()
                .any(|message| message.role == "user"),
            "{agent:?}"
        );
        assert!(
            projection
                .entries
                .iter()
                .any(|entry| entry.kind == "tool_call"),
            "{agent:?}"
        );
        assert!(
            projection
                .entries
                .iter()
                .any(|entry| entry.kind == "tool_result"),
            "{agent:?}"
        );
    }
}
