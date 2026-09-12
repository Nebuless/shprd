use serde_json::json;
use shprd_history::{Agent, FileMeta, HistoryService, HostConfig, PaneMetadata, Update};

fn pane(agent: Agent, path: &str) -> PaneMetadata {
    PaneMetadata::new("pane-1", "workspace-1", "tab-1", agent, path)
}

#[test]
fn pi_fixture_projects_v1_v2_lazy_tool_and_atif() {
    let source = [
        json!({"type":"session","version":3,"id":"pi-session","timestamp":"2026-07-24T00:00:00.000Z"}),
        json!({"type":"message","timestamp":"2026-07-24T00:00:01.000Z","message":{"role":"user","content":[{"type":"text","text":"inspect"}]}}),
        json!({"type":"message","timestamp":"2026-07-24T00:00:02.000Z","message":{"role":"assistant","model":"fixture-model","usage":{"input":4,"cacheRead":1,"output":2},"content":[{"type":"toolCall","id":"call-1","name":"read","arguments":{"path":"README.md"}},{"type":"text","text":"done"}]}}),
        json!({"type":"message","timestamp":"2026-07-24T00:00:03.000Z","message":{"role":"toolResult","toolCallId":"call-1","toolName":"read","content":[{"type":"text","text":"file contents"}]}}),
    ]
    .into_iter()
    .map(|record| record.to_string())
    .collect::<Vec<_>>()
    .join("\n")
        + "\n";
    let service = HistoryService::new(HostConfig::default());
    let meta = FileMeta::fixture("/fixture/pi.jsonl", &source, 1);
    let projection = service
        .project_text(
            &pane(Agent::Pi, &meta.path.to_string_lossy()),
            meta,
            &source,
        )
        .unwrap();
    assert_eq!(projection.v1_messages.len(), 2);
    assert_eq!(projection.v1_messages[0].text, "inspect");
    assert_eq!(projection.atif.schema_version, "ATIF-v1.7");
    assert_eq!(projection.atif.agent.name, "pi");
    assert_eq!(
        projection
            .atif
            .final_metrics
            .as_ref()
            .unwrap()
            .total_prompt_tokens,
        Some(5)
    );
    let snapshot = service.history_snapshot(&projection);
    let Update::Snapshot { entries, .. } = snapshot else {
        panic!("history snapshot expected");
    };
    let tool = entries.iter().find(|entry| entry.role == "tool").unwrap();
    assert_eq!(tool.text, "");
    assert!(tool.text_bytes.unwrap() > 0);
    assert!(
        service
            .entry(&projection, &tool.id)
            .unwrap()
            .text
            .contains("README.md")
            || service
                .entry(&projection, &tool.id)
                .unwrap()
                .text
                .contains("file contents")
    );
    let export = service.atif_json(&projection).unwrap();
    assert_eq!(export["schema_version"], "ATIF-v1.7");
}
