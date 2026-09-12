#![cfg(unix)]
use shprd_history::{HostConfig, RemoteFiles};
use std::os::unix::fs::PermissionsExt;

#[tokio::test]
async fn remote_output_caps_apply_to_both_process_streams() {
    for (size, redirect) in [(2 * 1024 * 1024 + 1, ""), (64 * 1024 + 1, ">&2")] {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("ssh-fixture");
        std::fs::write(
            &executable,
            format!("#!/bin/sh\nhead -c {size} /dev/zero {redirect}\n"),
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let remote = RemoteFiles::new("fixture-host", executable, HostConfig::default()).unwrap();
        let result = remote
            .read_text(std::path::Path::new("/fixture.jsonl"))
            .await;
        assert!(matches!(result, Err(shprd_history::Error::Invalid(_))));
    }
}

#[tokio::test]
async fn isolated_ssh_executable_reads_only_explicit_absolute_file_path() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory
        .path()
        .join("session with spaces and ' quote.jsonl");
    std::fs::write(&path, "{\"type\":\"session\"}\n").unwrap();
    let executable = directory.path().join("ssh-fixture");
    // OpenSSH joins remote argv into one command interpreted by the login shell.
    let script = r#"#!/bin/sh
while [ "$1" != "--" ]; do shift; done
shift 2
exec env -i PATH="$PATH" sh -c "$*"
"#;
    std::fs::write(&executable, script).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();

    let remote = RemoteFiles::new("fixture-host", executable, HostConfig::default()).unwrap();
    let bytes = remote.read_text(&path).await.unwrap();
    assert_eq!(bytes, "{\"type\":\"session\"}\n");
    assert!(
        remote
            .read_text(std::path::Path::new("relative"))
            .await
            .is_err()
    );
}
