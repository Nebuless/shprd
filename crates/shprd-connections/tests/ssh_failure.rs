#![cfg(unix)]
use shprd_connections::{Profile, SshTunnel};
use std::os::unix::fs::PermissionsExt;
fn profile(infer: bool) -> Profile {
    Profile::parse(&format!(r#"{{"id":"fixture","label":"Fixture","type":"ssh","ssh_destination":"fixture-only","remote_control_socket_path":"{}","remote_client_socket_path":"{}","auto_connect":false}}"#, if infer { "" } else { "/tmp/c" }, if infer { "" } else { "/tmp/r" })).unwrap()
}
#[tokio::test]
async fn early_auth_failure_classifies_and_cleans_private_directory() {
    let dir = tempfile::tempdir().unwrap();
    let executable = dir.path().join("ssh");
    std::fs::write(
        &executable,
        "#!/bin/sh\nprintf 'Permission denied (publickey)\\n' >&2\nexit 255\n",
    )
    .unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut tunnel = SshTunnel::spawn(&profile(false), &executable)
        .await
        .unwrap();
    let runtime_dir = tunnel.paths().control.parent().unwrap().to_owned();
    assert!(matches!(
        tunnel.wait_ready().await,
        Err(shprd_connections::Error::Runtime {
            retryable: false,
            ..
        })
    ));
    tunnel.stop().await.unwrap();
    assert!(!runtime_dir.exists());
}
#[tokio::test]
async fn inferred_remote_home_rejects_forwarding_metacharacters() {
    let dir = tempfile::tempdir().unwrap();
    let executable = dir.path().join("ssh");
    std::fs::write(&executable, "#!/bin/sh\nprintf '/home/user:injected'\n").unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(SshTunnel::spawn(&profile(true), &executable).await.is_err());
}
#[tokio::test]
async fn bounded_remote_home_rejects_oversized_stdout() {
    let dir = tempfile::tempdir().unwrap();
    let executable = dir.path().join("ssh");
    std::fs::write(
        &executable,
        "#!/bin/sh\nprintf '/'\ni=0; while [ $i -lt 5000 ]; do printf x; i=$((i+1)); done\n",
    )
    .unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(SshTunnel::spawn(&profile(true), &executable).await.is_err());
}
