use shprd_history::{Agent, HistoryService, HostConfig, LocalFiles, PaneMetadata};

#[test]
fn real_local_fixture_imports_and_exports_exact_fields() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("pi.jsonl");
    let source = "{\"type\":\"session\",\"id\":\"fixture-session\"}\n{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"inspect\"}}\n";
    std::fs::write(&path, source).unwrap();
    let file = LocalFiles::metadata(&path).unwrap().unwrap();
    let text = LocalFiles::read_text(&path).unwrap();
    assert_eq!(
        LocalFiles::read_prefix(&path, 8).unwrap(),
        source.as_bytes()[..8]
    );
    let service = HistoryService::new(HostConfig::default());
    let projection = service
        .project_text(
            &PaneMetadata::new("pane", "workspace", "tab", Agent::Pi, &path),
            file,
            &text,
        )
        .unwrap();
    let atif = service.atif_json(&projection).unwrap();
    assert_eq!(atif["schema_version"], "ATIF-v1.7");
    assert_eq!(atif["session_id"], "fixture-session");
    assert_eq!(atif["agent"]["name"], "pi");
    assert_eq!(projection.v1_messages[0].text, "inspect");
}
