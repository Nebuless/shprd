use serde_json::json;
use shprd_history::{Agent, FileMeta, HistoryService, HostConfig, MAX_PREVIEW_BYTES, PaneMetadata};

#[test]
fn session_view_clamps_utf8_preview_and_exports_exact_atif_schema() {
    let text = format!(
        "{}\n",
        json!({"type":"message","message":{"role":"user","content":"hello"}})
    );
    let service = HistoryService::new(HostConfig::default());
    let file = FileMeta::fixture("/fixture/pi.jsonl", &text, 1);
    let projection = service
        .project_text(
            &PaneMetadata::new("pane", "workspace", "tab", Agent::Pi, "/fixture/pi.jsonl"),
            file,
            &text,
        )
        .unwrap();
    let preview = "x".repeat(MAX_PREVIEW_BYTES - 1) + "émore";
    let session = service
        .session_view(&projection, Some(&preview), true)
        .unwrap();
    assert!(session.truncated);
    assert_eq!(session.stats.turns, 1);
    assert_eq!(
        session.trajectory.as_ref().unwrap()["schema_version"],
        "ATIF-v1.7"
    );
}
