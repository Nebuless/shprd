#![cfg(unix)]
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixListener,
};

#[tokio::test]
async fn catalog_requires_authenticated_live_state() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let owner = directory.path().join("0123456789abcdef0123456789abcdef");
    std::fs::create_dir(&owner).unwrap();
    std::fs::set_permissions(&owner, std::fs::Permissions::from_mode(0o700)).unwrap();
    let endpoint = owner.join("control.sock");
    let listener = UnixListener::bind(&endpoint).unwrap();
    std::fs::set_permissions(&endpoint, std::fs::Permissions::from_mode(0o600)).unwrap();
    let discovery = owner.join("attachment.json");
    std::fs::write(&discovery, json!({"version":1,"session_id":"atomic:fixture","agent":"atomic","endpoint":endpoint,"token":"a".repeat(64)}).to_string()).unwrap();
    std::fs::set_permissions(&discovery, std::fs::Permissions::from_mode(0o600)).unwrap();
    let probe = async {
        let (socket, _) = listener.accept().await.unwrap();
        let mut socket = BufReader::new(socket);
        let mut line = String::new();
        socket.read_line(&mut line).await.unwrap();
        let request: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(request["token"], "a".repeat(64));
        assert_eq!(request["session_id"], "atomic:fixture");
        assert_eq!(request["command"]["type"], "get_state");
        let response = json!({"id":request["id"],"result":{"id":"atomic:fixture","agent":"atomic","name":"Fixture","cwd":"/fixture","connected":true,"busy":true}});
        socket
            .get_mut()
            .write_all(format!("{response}\n").as_bytes())
            .await
            .unwrap();
    };
    let (sessions, ()) = tokio::time::timeout(Duration::from_secs(3), async {
        tokio::join!(shprd_agent::list(directory.path()), probe)
    })
    .await
    .unwrap();
    assert_eq!(sessions.unwrap()["sessions"][0]["id"], "atomic:fixture");
}

#[test]
fn dialog_command_roundtrips_public_wire_shape() {
    let value = json!({"type":"ui_dialog","kind":"input","title":"Fixture"});
    let command: shprd_agent::Command = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(command).unwrap(), value);
}

#[tokio::test]
async fn refuses_public_discovery_and_symlinks() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(
        shprd_agent::list(directory.path()).await,
        Err(shprd_agent::Error::UnsafeDiscovery)
    ));
    let link = directory.path().join("link");
    std::os::unix::fs::symlink(directory.path(), &link).unwrap();
    assert!(matches!(
        shprd_agent::list(&link).await,
        Err(shprd_agent::Error::UnsafeDiscovery)
    ));
}
