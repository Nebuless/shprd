#![cfg(unix)]
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use shprd_host::{auth::Auth, host};
use std::{os::unix::fs::PermissionsExt, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, UnixListener},
};

#[tokio::test]
async fn native_subscription_streams_after_command_reply() -> Result<(), Box<dyn std::error::Error>>
{
    let directory = tempfile::tempdir()?;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    let owner = directory.path().join("0123456789abcdef0123456789abcdef");
    std::fs::create_dir(&owner)?;
    std::fs::set_permissions(&owner, std::fs::Permissions::from_mode(0o700))?;
    let endpoint = owner.join("control.sock");
    let native = UnixListener::bind(&endpoint)?;
    std::fs::set_permissions(&endpoint, std::fs::Permissions::from_mode(0o600))?;
    let descriptor = owner.join("attachment.json");
    std::fs::write(&descriptor, json!({"version":1,"session_id":"atomic:fixture","agent":"atomic","endpoint":endpoint,"token":"a".repeat(64)}).to_string())?;
    std::fs::set_permissions(&descriptor, std::fs::Permissions::from_mode(0o600))?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let router = host::configured_router_with_attachments(
        directory.path().join("missing.sock"),
        directory.path().to_path_buf(),
        Auth::new(false, String::new())?,
        Some(directory.path().to_path_buf()),
    );
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let (emit, emitted) = tokio::sync::oneshot::channel();
    let server = async {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .map_err(Into::into)
    };
    let runtime = async {
        let (socket, _) = native.accept().await?;
        let mut socket = BufReader::new(socket);
        let mut line = String::new();
        socket.read_line(&mut line).await?;
        let request: Value = serde_json::from_str(&line)?;
        assert_eq!(request["command"]["type"], "get_state");
        assert_eq!(request["token"], "a".repeat(64));
        socket.get_mut().write_all(format!("{}\n",json!({"id":request["id"],"result":{"id":"atomic:fixture","agent":"atomic","name":"Fixture","cwd":"/fixture","connected":true,"busy":false}})).as_bytes()).await?;
        emitted.await?;
        socket.get_mut().write_all(format!("{}\n",json!({"agent_event":{"session_id":"atomic:fixture","event":{"type":"agent_start"}}})).as_bytes()).await?;
        line.clear();
        assert_eq!(socket.read_line(&mut line).await?, 0);
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    let browser = async {
        let (mut socket, _) =
            tokio_tungstenite::connect_async(format!("ws://{address}/ws")).await?;
        socket.next().await.ok_or("missing hello")??;
        socket.send(tokio_tungstenite::tungstenite::Message::Text(json!({"id":"watch","method":"agent_control.subscribe","params":{"session_id":"atomic:fixture"},"connection_id":"legacy-default","connection_generation":1}).to_string().into())).await?;
        let response: Value =
            serde_json::from_str(socket.next().await.ok_or("missing response")??.to_text()?)?;
        assert_eq!(response["result"], json!({"ok":true}));
        assert_eq!(response["connection_generation"], 1);
        emit.send(()).map_err(|()| "lost event trigger")?;
        let event: Value =
            serde_json::from_str(socket.next().await.ok_or("missing event")??.to_text()?)?;
        assert_eq!(
            event,
            json!({"connection_id":"legacy-default","connection_generation":1,"event":"agent_control.event","data":{"agent_event":{"session_id":"atomic:fixture","event":{"type":"agent_start"}}}})
        );
        socket.close(None).await?;
        drop(socket);
        stop.send(()).map_err(|()| "lost shutdown")?;
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        tokio::try_join!(server, runtime, browser)
    })
    .await??;
    Ok(())
}
