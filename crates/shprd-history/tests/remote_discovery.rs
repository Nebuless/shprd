#![cfg(unix)]
use shprd_history::{Agent, HostConfig, RemoteFiles};
use std::{os::unix::fs::PermissionsExt, process::Command};

#[tokio::test]
async fn isolated_ssh_discovery_uses_bounded_find_and_returns_newest_match() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("sessions");
    let old = root.join("old/session-a.jsonl");
    let newest = root.join("new/session-a.jsonl");
    std::fs::create_dir_all(old.parent().unwrap()).unwrap();
    std::fs::create_dir_all(newest.parent().unwrap()).unwrap();
    std::fs::write(&old, "old").unwrap();
    std::fs::write(&newest, "new").unwrap();
    assert!(
        Command::new("touch")
            .args(["-t", "202601010101", old.to_str().unwrap()])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new("touch")
            .args(["-t", "202601010102", newest.to_str().unwrap()])
            .status()
            .unwrap()
            .success()
    );
    let executable = directory.path().join("ssh-fixture");
    std::fs::write(
        &executable,
        r#"#!/bin/sh
while [ "$1" != "--" ]; do shift; done
shift 2
exec env -i PATH="$PATH" sh -c "$*"
"#,
    )
    .unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let remote = RemoteFiles::new("fixture-host", executable, HostConfig::default()).unwrap();
    let found = remote
        .find_session(Agent::Pi, &root, "session-a", 8, 200)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found, newest);
    assert!(
        remote
            .find_session(Agent::Pi, &root, "../bad", 8, 200)
            .await
            .is_err()
    );
}
