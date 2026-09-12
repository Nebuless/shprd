use serde_json::json;
use shprd_history::{Agent, FileMeta, HistoryService, HostConfig, PaneMetadata, Update};

fn source(text: &str) -> String {
    json!({"type":"message","timestamp":"2026-01-01T00:00:00Z","message":{"role":"user","content":text}}).to_string() + "\n"
}
fn pane() -> PaneMetadata {
    PaneMetadata::new("p", "w", "t", Agent::Pi, "/fixture/history.jsonl")
}

#[test]
fn cache_returns_delta_then_snapshot_after_identity_replacement_and_evicts_oversize() {
    let service = HistoryService::new(HostConfig {
        cache_entries: 1,
        cache_bytes: 1024,
        ..Default::default()
    });
    let first = source("one");
    let meta = FileMeta::fixture("/fixture/history.jsonl", &first, 1);
    let (_, snapshot) = service
        .history(&pane(), meta.clone(), &first, None)
        .unwrap();
    let Update::Snapshot { cursor, .. } = snapshot else {
        panic!("initial snapshot expected")
    };
    let second = first.clone() + &source("two");
    let mut changed = meta.clone();
    changed.mtime_ms = 2;
    changed.size = Some(second.len() as u64);
    let (_, delta) = service
        .history(&pane(), changed.clone(), &second, Some(&cursor))
        .unwrap();
    let Update::Delta { upserts, .. } = delta else {
        panic!("delta expected")
    };
    assert_eq!(upserts.len(), 1);
    changed.identity = Some("replacement".into());
    let (_, reset) = service
        .history(&pane(), changed, &second, Some(&cursor))
        .unwrap();
    assert!(matches!(reset, Update::Snapshot { .. }));
    let oversized = source(&"x".repeat(4000));
    let big = FileMeta::fixture("/fixture/history.jsonl", &oversized, 5);
    let (_, first_update) = service
        .history(&pane(), big.clone(), &oversized, None)
        .unwrap();
    let Update::Snapshot { cursor, .. } = first_update else {
        panic!("snapshot expected")
    };
    let (_, next_update) = service
        .history(&pane(), big, &oversized, Some(&cursor))
        .unwrap();
    assert!(matches!(next_update, Update::Snapshot { .. }));
}

#[test]
fn malformed_trailing_jsonl_is_ignored_without_mutating_source() {
    let service = HistoryService::new(Default::default());
    let text = source("ok") + "{\"partial\":";
    let projection = service
        .project_text(
            &pane(),
            FileMeta::fixture("/fixture/history.jsonl", &text, 1),
            &text,
        )
        .unwrap();
    assert_eq!(projection.v1_messages[0].text, "ok");
}
