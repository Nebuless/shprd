use shprd_history::{
    Agent, HostConfig, PaneMetadata, ResolveStatus, Resolver, SessionKind, SessionRef,
};
use std::fs;

#[test]
fn resolves_known_id_with_bounded_local_discovery_and_refuses_traversal() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(".pi/agent/sessions/project/2026_ok.jsonl");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "{}\n").unwrap();
    let config = HostConfig {
        home: Some(root.path().to_owned()),
        ..Default::default()
    };
    let resolver = Resolver::new(config);
    let mut pane = PaneMetadata::new("p", "w", "t", Agent::Pi, "/unresolved");
    pane.session = Some(SessionRef {
        source: "fixture".into(),
        agent: Agent::Pi,
        kind: SessionKind::Id,
        value: "ok".into(),
    });
    let found = resolver.resolve(&pane).unwrap();
    assert_eq!(found.status, ResolveStatus::Ok);
    assert_eq!(found.file.unwrap().path, path);
    pane.session.as_mut().unwrap().value = "../bad".into();
    assert!(resolver.resolve(&pane).is_err());
}
#[test]
fn explicit_path_requires_regular_readonly_file_and_no_write_surface() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("session.jsonl");
    fs::write(&file, "{}\n").unwrap();
    let resolver = Resolver::new(HostConfig::default());
    let mut pane = PaneMetadata::new("p", "w", "t", Agent::Codex, &file);
    pane.session = Some(SessionRef {
        source: "fixture".into(),
        agent: Agent::Codex,
        kind: SessionKind::Path,
        value: file.to_string_lossy().into(),
    });
    assert_eq!(resolver.resolve(&pane).unwrap().status, ResolveStatus::Ok);
    fs::remove_file(&file).unwrap();
    fs::create_dir(&file).unwrap();
    assert_eq!(
        resolver.resolve(&pane).unwrap().status,
        ResolveStatus::MissingFile
    );
}
