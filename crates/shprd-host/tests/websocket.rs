use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use shprd_host::{auth::Auth, host};
use tokio::net::TcpListener;

#[cfg(unix)]
#[tokio::test]
async fn pending_rpc_does_not_block_ping_and_disconnect_closes_downstream()
-> Result<(), Box<dyn std::error::Error>> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
    // Given a downstream holding one RPC open without a reply.
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("control.sock");
    let downstream = tokio::net::UnixListener::bind(&path)?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let router = host::configured_router(
        path,
        directory.path().to_path_buf(),
        Auth::new(false, String::new())?,
    );
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let (accepted, called) = tokio::sync::oneshot::channel();
    let (closed, disconnected) = tokio::sync::oneshot::channel();
    let server = async {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .map_err(Into::into)
    };
    let downstream = async {
        let (socket, _) = downstream.accept().await?;
        let mut socket = BufReader::new(socket);
        let mut line = String::new();
        socket.read_line(&mut line).await?;
        accepted.send(()).map_err(|()| "lost call observer")?;
        let mut byte = [0];
        assert_eq!(socket.read(&mut byte).await?, 0);
        closed.send(()).map_err(|()| "lost disconnect observer")?;
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    let client = async {
        let (mut socket, _) =
            tokio_tungstenite::connect_async(format!("ws://{address}/ws")).await?;
        socket.next().await.ok_or("missing hello")??;
        socket
            .send(tokio_tungstenite::tungstenite::Message::Text(
                json!({"id":"slow","method":"workspace.list"})
                    .to_string()
                    .into(),
            ))
            .await?;
        called.await?;
        // When a ping is sent before the slow request resolves, then it replies independently.
        socket
            .send(tokio_tungstenite::tungstenite::Message::Text(
                json!({"id":"ping","method":"bridge.ping"})
                    .to_string()
                    .into(),
            ))
            .await?;
        let reply = tokio::time::timeout(std::time::Duration::from_secs(1), socket.next())
            .await
            .map_err(|_| "bridge ping blocked behind downstream RPC")?
            .ok_or("missing ping")??;
        assert_eq!(
            serde_json::from_str::<Value>(reply.to_text()?)?["id"],
            "ping"
        );
        socket.close(None).await?;
        drop(socket);
        disconnected.await?;
        stop.send(()).map_err(|()| "lost server shutdown")?;
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        tokio::try_join!(server, downstream, client)
    })
    .await??;
    Ok(())
}

#[tokio::test]
async fn websocket_preserves_hello_and_rpc_contract() -> Result<(), Box<dyn std::error::Error>> {
    // Given a real HTTP listener with no running downstream.
    let directory = tempfile::tempdir()?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let router = host::configured_router(
        directory.path().join("missing.sock"),
        directory.path().to_path_buf(),
        Auth::new(false, String::new())?,
    );
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = async {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
    };
    let client = async {
        // When connecting and sending a bridge-global request.
        let (mut socket, _) =
            tokio_tungstenite::connect_async(format!("ws://{address}/ws")).await?;
        let hello: Value =
            serde_json::from_str(socket.next().await.ok_or("missing hello")??.to_text()?)?;
        assert_eq!(hello["bridge_protocol_version"], 2);
        assert_eq!(hello["default_connection_id"], "legacy-default");
        socket
            .send(tokio_tungstenite::tungstenite::Message::Text(
                json!({"id":"one","method":"bridge.ping"})
                    .to_string()
                    .into(),
            ))
            .await?;
        // Then response keeps browser request identity and exact result shape.
        let response: Value =
            serde_json::from_str(socket.next().await.ok_or("missing response")??.to_text()?)?;
        assert_eq!(response, json!({"id":"one","result":{"ok":true}}));
        socket
            .send(tokio_tungstenite::tungstenite::Message::Text(
                json!({"id":"bad-global","method":"bridge.ping","connection_id":"legacy-default"})
                    .to_string()
                    .into(),
            ))
            .await?;
        let rejected: Value =
            serde_json::from_str(socket.next().await.ok_or("missing rejection")??.to_text()?)?;
        assert!(rejected.get("error").is_some());
        socket
            .send(tokio_tungstenite::tungstenite::Message::Text(
                json!({"id":"catalog","method":"connections.list"})
                    .to_string()
                    .into(),
            ))
            .await?;
        let catalog: Value =
            serde_json::from_str(socket.next().await.ok_or("missing catalog")??.to_text()?)?;
        assert_eq!(catalog["result"]["default_connection_id"], "legacy-default");
        assert_eq!(catalog["result"]["connections"][0]["state"], "error");
        socket.close(None).await?;
        drop(socket);
        stop.send(()).map_err(|()| "shutdown lost")?;
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::try_join!(client, async { server.await.map_err(Into::into) })
    })
    .await??;
    Ok(())
}
