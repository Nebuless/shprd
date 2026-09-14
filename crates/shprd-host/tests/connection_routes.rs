use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{process::Stdio, time::Duration};
use tokio::sync::oneshot;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
};

#[cfg(unix)]
#[tokio::test]
async fn disconnect_cancels_pending_profile_update_without_profile_lock()
-> Result<(), Box<dyn std::error::Error>> {
    use shprd_connections::{
        ConnectionId, Manager, Profile, ProfileService, Runtime, RuntimeContext, RuntimeFuture,
        SocketPaths, Store,
    };
    use shprd_host::{auth::Auth, host};
    use std::{os::unix::fs::PermissionsExt, sync::Arc};
    use tokio::sync::Notify;
    struct Held {
        started: Arc<Notify>,
        release: Arc<Notify>,
        cancelled: Arc<Notify>,
    }
    impl Runtime for Held {
        fn start<'a>(&'a self, context: &'a RuntimeContext) -> RuntimeFuture<'a, SocketPaths> {
            Box::pin(async move {
                self.started.notify_one();
                tokio::select! {biased; _ = context.cancelled() => {self.cancelled.notify_one();return Err(shprd_connections::Error::Stale);}, _ = self.release.notified() => {}};
                Ok(SocketPaths {
                    control: "/replacement/control".into(),
                    render: "/replacement/render".into(),
                })
            })
        }
        fn stop(&self) -> RuntimeFuture<'_, ()> {
            Box::pin(async {
                self.cancelled.notify_one();
                Ok(())
            })
        }
    }
    // Given a persisted disconnected profile whose replacement startup is held by an exact signal.
    let directory = tempfile::tempdir()?;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    let path = directory.path().join("connections.json");
    let id = ConnectionId::parse("saved")?;
    let manager = Arc::new(Manager::new(ConnectionId::parse("legacy-default")?));
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let cancelled = Arc::new(Notify::new());
    let runtime = Arc::new(Held {
        started: Arc::clone(&started),
        release: Arc::clone(&release),
        cancelled: Arc::clone(&cancelled),
    });
    let mut profiles = ProfileService::load(
        Store::new(path.clone())?,
        Profile::legacy("/legacy/control", "/legacy/render")?,
        true,
        Arc::clone(&manager),
        Arc::new(move |_| {
            let runtime = Arc::clone(&runtime);
            Arc::new(move |_| Ok(runtime.clone()))
        }),
    )?;
    let original = json!({"id":"saved","label":"Original","type":"local","auto_connect":false,"control_socket_path":"/original/control","client_socket_path":"/original/render"});
    profiles.create(Profile::from_value(original)?).await?;
    let replacement = json!({"id":"saved","label":"Replacement","type":"local","auto_connect":true,"control_socket_path":"/replacement/control","client_socket_path":"/replacement/render"});
    let router = host::configured_router_with_profiles(
        "/legacy/control".into(),
        directory.path().into(),
        Auth::new(false, String::new())?,
        None,
        profiles,
        Arc::clone(&manager),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = async {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .map_err(Into::into)
    };
    let client = async {
        let (mut writer, _) =
            tokio_tungstenite::connect_async(format!("ws://{address}/ws")).await?;
        writer.next().await.ok_or("writer hello")??;
        writer.send(tokio_tungstenite::tungstenite::Message::Text(json!({"id":"update","method":"connections.update","params":{"id":"saved","profile":replacement}}).to_string().into())).await?;
        started.notified().await;
        let persisted: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
        assert_eq!(persisted["profiles"][0], replacement);
        // When another browser disconnects the connection while update owns the profile lock.
        let (mut reader, _) =
            tokio_tungstenite::connect_async(format!("ws://{address}/ws")).await?;
        reader.next().await.ok_or("reader hello")??;
        reader
            .send(tokio_tungstenite::tungstenite::Message::Text(
                json!({"id":"cancel","method":"connections.disconnect","params":{"id":"saved"}})
                    .to_string()
                    .into(),
            ))
            .await?;
        // Then cleanup starts before the held startup is released, and rollback stays coherent.
        tokio::time::timeout(Duration::from_secs(1), cancelled.notified())
            .await
            .map_err(|_| "disconnect blocked behind profile mutation")?;
        let cancelled_reply: Value =
            serde_json::from_str(reader.next().await.ok_or("cancel")??.to_text()?)?;
        assert!(cancelled_reply.get("error").is_none(), "{cancelled_reply}");
        let updated: Value =
            serde_json::from_str(writer.next().await.ok_or("update")??.to_text()?)?;
        assert!(updated.get("error").is_some(), "{updated}");
        let persisted: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
        assert_eq!(
            persisted["profiles"][0]["control_socket_path"],
            "/original/control"
        );
        assert_eq!(
            manager.status(&id)?.state,
            shprd_connections::State::Disconnected
        );
        reader.close(None).await?;
        writer.close(None).await?;
        drop(reader);
        drop(writer);
        stop.send(()).map_err(|()| "stop")?;
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::try_join!(server, client)
    })
    .await;
    manager.stop_all().await?;
    result??;
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn profile_update_commits_after_requesting_websocket_closes()
-> Result<(), Box<dyn std::error::Error>> {
    use shprd_connections::{
        ConnectionId, Manager, Profile, ProfileService, Runtime, RuntimeContext, RuntimeFuture,
        SocketPaths, Store,
    };
    use shprd_host::{auth::Auth, host};
    use std::{os::unix::fs::PermissionsExt, sync::Arc};
    use tokio::sync::Notify;
    struct Held {
        started: Arc<Notify>,
        release: Arc<Notify>,
    }
    impl Runtime for Held {
        fn start<'a>(&'a self, _: &'a RuntimeContext) -> RuntimeFuture<'a, SocketPaths> {
            Box::pin(async move {
                self.started.notify_one();
                self.release.notified().await;
                Ok(SocketPaths {
                    control: "/replacement/control".into(),
                    render: "/replacement/render".into(),
                })
            })
        }
        fn stop(&self) -> RuntimeFuture<'_, ()> {
            Box::pin(async { Ok(()) })
        }
    }
    // Given a persisted disconnected profile whose replacement startup is held by an exact signal.
    let directory = tempfile::tempdir()?;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    let path = directory.path().join("connections.json");
    let id = ConnectionId::parse("saved")?;
    let manager = Arc::new(Manager::new(ConnectionId::parse("legacy-default")?));
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let runtime = Arc::new(Held {
        started: Arc::clone(&started),
        release: Arc::clone(&release),
    });
    let mut profiles = ProfileService::load(
        Store::new(path.clone())?,
        Profile::legacy("/legacy/control", "/legacy/render")?,
        true,
        Arc::clone(&manager),
        Arc::new(move |_| {
            let runtime = Arc::clone(&runtime);
            Arc::new(move |_| Ok(runtime.clone()))
        }),
    )?;
    let original = json!({"id":"saved","label":"Original","type":"local","auto_connect":false,"control_socket_path":"/original/control","client_socket_path":"/original/render"});
    profiles.create(Profile::from_value(original)?).await?;
    let replacement = json!({"id":"saved","label":"Replacement","type":"local","auto_connect":true,"control_socket_path":"/replacement/control","client_socket_path":"/replacement/render"});
    let router = host::configured_router_with_profiles(
        "/legacy/control".into(),
        directory.path().into(),
        Auth::new(false, String::new())?,
        None,
        profiles,
        Arc::clone(&manager),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = async {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .map_err(Into::into)
    };
    let client = async {
        let (mut writer, _) =
            tokio_tungstenite::connect_async(format!("ws://{address}/ws")).await?;
        writer.next().await.ok_or("writer hello")??;
        writer.send(tokio_tungstenite::tungstenite::Message::Text(json!({"id":"update","method":"connections.update","params":{"id":"saved","profile":replacement}}).to_string().into())).await?;
        started.notified().await;
        let persisted: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
        assert_eq!(persisted["profiles"][0], replacement);
        // When the requester disconnects after save but before runtime startup completes.
        writer.close(None).await?;
        let _ = writer.next().await;
        drop(writer);
        release.notify_one();
        // Then another client observes committed metadata and the matching ready runtime.
        let (mut reader, _) =
            tokio_tungstenite::connect_async(format!("ws://{address}/ws")).await?;
        reader.next().await.ok_or("reader hello")??;
        reader
            .send(tokio_tungstenite::tungstenite::Message::Text(
                json!({"id":"list","method":"connections.list"})
                    .to_string()
                    .into(),
            ))
            .await?;
        let result: Value = serde_json::from_str(reader.next().await.ok_or("list")??.to_text()?)?;
        let profile = result["result"]["connections"]
            .as_array()
            .ok_or("profiles")?
            .iter()
            .find(|item| item["id"] == "saved")
            .ok_or("saved")?;
        assert_eq!(profile["label"], "Replacement", "{result}");
        assert_eq!(profile["state"], "ready", "{result}");
        assert_eq!(
            manager.lease(&id)?.paths.control,
            std::path::PathBuf::from("/replacement/control")
        );
        reader.close(None).await?;
        drop(reader);
        stop.send(()).map_err(|()| "stop")?;
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::try_join!(server, client)
    })
    .await;
    manager.stop_all().await?;
    result??;
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn native_binary_serves_management_while_profile_is_connecting()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;
    // Given a downstream that accepts control requests but never completes its ping.
    let directory = tempfile::tempdir()?;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    let control = directory.path().join("control.sock");
    let listener = tokio::net::UnixListener::bind(&control)?;
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_shprd"));
    for name in [
        "HOST",
        "PORT",
        "HERDR_GUI_PASSWORD",
        "HERDR_SESSION",
        "HERDR_SOCKET_PATH",
        "HERDR_CLIENT_SOCKET_PATH",
        "SHPRD_CONFIG_DIR",
        "SHPRD_AGENT_DIR",
    ] {
        command.env_remove(name);
    }
    let mut process = command
        .env("HOME", directory.path())
        .env(
            "HERDR_GUI_CONNECTIONS_PATH",
            directory.path().join("connections.json"),
        )
        .args(["--host", "127.0.0.1", "--port", "0"])
        .arg("--socket-path")
        .arg(&control)
        .arg("--client-socket-path")
        .arg(directory.path().join("render.sock"))
        .kill_on_drop(true)
        .stderr(Stdio::piped())
        .spawn()?;
    let result = async {
        let (accepted, _) =
            tokio::time::timeout(Duration::from_secs(5), listener.accept()).await??;
        let mut pending = BufReader::new(accepted);
        let mut ping = String::new();
        tokio::time::timeout(Duration::from_secs(5), pending.read_line(&mut ping)).await??;
        assert_eq!(serde_json::from_str::<Value>(&ping)?["method"], "ping");
        // When the native listener starts, management stays available without the ping reply.
        let mut startup = BufReader::new(process.stderr.take().ok_or("stderr")?);
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(2), startup.read_line(&mut line))
            .await
            .map_err(|_| "native listener waits for downstream startup")??;
        let address = line
            .trim()
            .strip_prefix("SHPRD_LISTENING=http://")
            .ok_or("listener address")?;
        let mut http = TcpStream::connect(address).await?;
        http.write_all(
            format!("GET /health HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await?;
        let mut http_response = String::new();
        http.read_to_string(&mut http_response).await?;
        assert!(http_response.starts_with("HTTP/1.1 200"), "{http_response}");
        println!("SURFACE_NATIVE_HTTP status=200");
        let (mut socket, _) =
            tokio_tungstenite::connect_async(format!("ws://{address}/ws")).await?;
        socket.next().await.ok_or("hello")??;
        socket
            .send(tokio_tungstenite::tungstenite::Message::Text(
                json!({"id":"profiles","method":"connections.list"})
                    .to_string()
                    .into(),
            ))
            .await?;
        let reply: Value =
            serde_json::from_str(socket.next().await.ok_or("profiles")??.to_text()?)?;
        assert_eq!(reply["result"]["connections"][0]["state"], "connecting");
        println!(
            "SURFACE_NATIVE_WS state={}",
            reply["result"]["connections"][0]["state"]
        );
        socket.close(None).await?;
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    let result = tokio::time::timeout(Duration::from_secs(10), result).await;
    process.kill().await?;
    process.wait().await?;
    result??;
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn native_binary_event_eof_retires_ready_connection() -> Result<(), Box<dyn std::error::Error>>
{
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir()?;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    let control = directory.path().join("control.sock");
    let render = directory.path().join("render.sock");
    let control_listener = tokio::net::UnixListener::bind(&control)?;
    let render_listener = tokio::net::UnixListener::bind(&render)?;
    let (subscription_ready_tx, subscription_ready_rx) = oneshot::channel::<()>();
    let (retire_tx, retire_rx) = oneshot::channel::<()>();
    let (retirement_done_tx, retirement_done_rx) = oneshot::channel::<()>();
    let fixture = tokio::spawn(async move {
        let result: Result<(), Box<dyn std::error::Error + Send + Sync>> = async {
            let (control_socket, _) = control_listener.accept().await?;
            let mut control_reader = BufReader::new(control_socket);
            let mut ping = String::new();
            control_reader.read_line(&mut ping).await?;
            let mut control_socket = control_reader.into_inner();
            control_socket
                .write_all(b"{\"id\":\"rpc\",\"result\":{\"protocol\":22}}\n")
                .await?;
            drop(control_socket);
            let (render_socket, _) = render_listener.accept().await?;
            let mut render_reader = BufReader::new(render_socket);
            let mut frame_length = [0_u8; 4];
            render_reader.read_exact(&mut frame_length).await?;
            let frame_length = u32::from_le_bytes(frame_length) as usize;
            if frame_length > 1024 * 1024 {
                return Err("render hello too large".into());
            }
            let mut render_hello = vec![0_u8; frame_length];
            render_reader.read_exact(&mut render_hello).await?;
            let mut render_socket = render_reader.into_inner();
            let response = bincode::encode_to_vec(
                (0_u32, 22_u32, 1_u32, Option::<String>::None),
                bincode::config::standard(),
            )
            .map_err(|error| format!("render handshake encode: {error}"))?;
            render_socket
                .write_all(&(response.len() as u32).to_le_bytes())
                .await?;
            render_socket.write_all(&response).await?;
            let (event_socket, _) = control_listener.accept().await?;
            let mut event_reader = BufReader::new(event_socket);
            let mut subscription = String::new();
            event_reader.read_line(&mut subscription).await?;
            assert!(subscription.contains("events.subscribe"), "{subscription}");
            let mut event_socket = event_reader.into_inner();
            event_socket
                .write_all(b"{\"id\":\"sub\",\"result\":{}}\n")
                .await?;
            subscription_ready_tx
                .send(())
                .map_err(|_| "subscription-ready receiver dropped")?;
            retire_rx.await.map_err(|_| "retirement signal dropped")?;
            drop(event_socket);
            retirement_done_tx
                .send(())
                .map_err(|_| "retirement-done receiver dropped")?;
            Ok(())
        }
        .await;
        result
    });
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_shprd"));
    for name in [
        "HOST",
        "PORT",
        "HERDR_GUI_PASSWORD",
        "HERDR_SESSION",
        "HERDR_SOCKET_PATH",
        "HERDR_CLIENT_SOCKET_PATH",
        "SHPRD_CONFIG_DIR",
        "SHPRD_AGENT_DIR",
    ] {
        command.env_remove(name);
    }
    let mut process = command
        .env("HOME", directory.path())
        .env(
            "HERDR_GUI_CONNECTIONS_PATH",
            directory.path().join("connections.json"),
        )
        .args(["--host", "127.0.0.1", "--port", "0"])
        .arg("--socket-path")
        .arg(&control)
        .arg("--client-socket-path")
        .arg(&render)
        .kill_on_drop(true)
        .stderr(Stdio::piped())
        .spawn()?;
    let process_id = process.id().ok_or("native process pid")?;
    let result = async {
        let mut startup = BufReader::new(process.stderr.take().ok_or("stderr")?);
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(5), startup.read_line(&mut line)).await??;
        let address = line
            .trim()
            .strip_prefix("SHPRD_LISTENING=http://")
            .ok_or("listener")?;
        let (mut socket, _) =
            tokio_tungstenite::connect_async(format!("ws://{address}/ws")).await?;
        socket.next().await.ok_or("hello")??;
        subscription_ready_rx
            .await
            .map_err(|_| "subscription not ready")?;
        socket
            .send(tokio_tungstenite::tungstenite::Message::Text(
                json!({"id":"ready","method":"connections.list"})
                    .to_string()
                    .into(),
            ))
            .await?;
        let ready: Value = serde_json::from_str(socket.next().await.ok_or("ready")??.to_text()?)?;
        assert_eq!(
            ready["result"]["connections"][0]["state"], "ready",
            "{ready}"
        );
        let generation = ready["result"]["connections"][0]["generation"]
            .as_u64()
            .ok_or("generation")?;
        println!("SURFACE_NATIVE_READY generation={generation}");
        retire_tx.send(()).map_err(|_| "retire receiver dropped")?;
        retirement_done_rx
            .await
            .map_err(|_| "retirement not complete")?;
        let mut http = TcpStream::connect(address).await?;
        http.write_all(
            format!(
                "GET /api/connections/legacy-default/herdr-info HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
            )
            .as_bytes(),
        )
        .await?;
        let mut http_response = String::new();
        http.read_to_string(&mut http_response).await?;
        assert!(http_response.starts_with("HTTP/1.1 503"), "{http_response}");
        println!("SURFACE_NATIVE_HTTP_RECONNECTING status=503");
        socket
            .send(tokio_tungstenite::tungstenite::Message::Text(
                json!({"id":"status","method":"connections.list"})
                    .to_string()
                    .into(),
            ))
            .await?;
        let status: Value = serde_json::from_str(socket.next().await.ok_or("status")??.to_text()?)?;
        let item = &status["result"]["connections"][0];
        assert_eq!(item["state"], "reconnecting", "{status}");
        assert!(item["generation"].as_u64().unwrap_or_default() > generation);
        println!(
            "SURFACE_NATIVE_RECONNECTING generation={}",
            item["generation"]
        );
        socket.send(tokio_tungstenite::tungstenite::Message::Text(json!({"id":"disconnect","method":"connections.disconnect","params":{"id":"legacy-default"}}).to_string().into())).await?;
        let disconnected: Value =
            serde_json::from_str(socket.next().await.ok_or("disconnect")??.to_text()?)?;
        assert_eq!(
            disconnected["result"]["state"], "disconnected",
            "{disconnected}"
        );
        println!("SURFACE_NATIVE_DISCONNECTED");
        socket.close(None).await?;
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    let result = tokio::time::timeout(Duration::from_secs(12), result).await;
    process.kill().await?;
    process.wait().await?;
    let fixture_completed = result.as_ref().is_ok_and(|result| result.is_ok());
    if fixture_completed {
        fixture
            .await
            .map_err(|error| format!("fixture join: {error}"))?
            .map_err(|error| error.to_string())?;
    } else {
        fixture.abort();
        let _ = fixture.await;
    }
    result??;
    println!(
        "SURFACE_NATIVE_CLEANUP process={} fixture={} sockets={}",
        process_id,
        if fixture_completed {
            "joined"
        } else {
            "aborted"
        },
        directory.path().display()
    );
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn native_binary_persists_created_connection() -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;
    // Use the Bun config contract, not only the registry-file override.
    let directory = tempfile::tempdir()?;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    let root = directory.path().to_path_buf();
    let config = root.join("config");
    let registry = config.join("connections.json");
    let control = root.join("control.sock");
    let render = root.join("render.sock");
    let control_listener = tokio::net::UnixListener::bind(&control)?;
    let render_listener = tokio::net::UnixListener::bind(&render)?;
    let profile = json!({
        "id":"qa-native", "label":"QA native", "type":"local", "auto_connect":false,
        "control_socket_path":control, "client_socket_path":render
    });
    let mut fixtures = tokio::task::JoinSet::new();
    let (subscribed, mut subscriptions) = tokio::sync::mpsc::channel(2);
    fixtures.spawn(async move {
        // Each explicit connect must complete a real ping and keep its event socket alive.
        for _ in 0..2 {
            let (stream, _) = control_listener.accept().await?;
            let mut stream = BufReader::new(stream);
            let mut line = String::new();
            stream.read_line(&mut line).await?;
            let request: Value = serde_json::from_str(&line)?;
            assert_eq!(request["method"], "ping");
            stream.get_mut().write_all(format!("{}\n", json!({"id":request["id"],"result":{"protocol":22,"version":"c001-fixture"}})).as_bytes()).await?;
            drop(stream);
            let (stream, _) = control_listener.accept().await?;
            let mut stream = BufReader::new(stream);
            line.clear();
            stream.read_line(&mut line).await?;
            let request: Value = serde_json::from_str(&line)?;
            assert_eq!(request["method"], "events.subscribe");
            stream.get_mut().write_all(format!("{}\n", json!({"id":request["id"],"result":{}})).as_bytes()).await?;
            subscribed.send(()).await?;
            // EOF acknowledges native disconnect/shutdown; no scheduling delay.
            let mut byte = [0];
            assert_eq!(stream.read(&mut byte).await?, 0);
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    });
    fixtures.spawn(async move {
        for _ in 0..2 {
            let (mut stream, _) = render_listener.accept().await?;
            let length = stream.read_u32_le().await?;
            assert!(length <= 1024, "oversized fixture hello");
            let mut hello = vec![0; usize::try_from(length)?];
            stream.read_exact(&mut hello).await?;
            assert_eq!(hello, shprd_host::render::hello(22, 80, 24)?);
            let welcome = bincode::encode_to_vec(
                (0_u32, 22_u32, 1_u32, None::<String>),
                bincode::config::standard(),
            )?;
            stream.write_u32_le(u32::try_from(welcome.len())?).await?;
            stream.write_all(&welcome).await?;
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    });
    let mut transcript = Vec::new();
    let mut processes = Vec::new();
    let result = async {
        for restart in [false, true] {
            let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_shprd"));
            command.env_clear().env("HOME", &root).env("USERPROFILE", &root)
                .env("APPDATA", &root).env("SHPRD_AGENT_DIR", root.join("agents"))
                .env("SHPRD_CONFIG_DIR", if restart { root.join("unused-config") } else { config.clone() });
            if restart {
                // Explicit file override wins over SHPRD_CONFIG_DIR and reloads the saved profile.
                command.env("HERDR_GUI_CONNECTIONS_PATH", &registry);
            }
            let mut process = command.args(["--host", "127.0.0.1", "--port", "0"])
                .arg("--socket-path").arg(root.join("missing-control.sock"))
                .arg("--client-socket-path").arg(root.join("missing-render.sock"))
                .kill_on_drop(true).stderr(Stdio::piped()).spawn()?;
            let pid = process.id().ok_or("native pid")?;
            let outcome = tokio::time::timeout(Duration::from_secs(10), async {
                let mut startup = BufReader::new(process.stderr.take().ok_or("stderr")?);
                let mut line = String::new();
                startup.read_line(&mut line).await?;
                let address = line.trim().strip_prefix("SHPRD_LISTENING=http://").ok_or("listener")?;
                let address: std::net::SocketAddr = address.parse()?;
                assert!(address.ip().is_loopback());
                assert_ne!(address.port(), 0);
                let (mut socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}/ws")).await?;
                let hello: Value = serde_json::from_str(socket.next().await.ok_or("hello")??.to_text()?)?;
                assert_eq!(hello["hello"], true);
                transcript.push(json!({"restart":restart,"pid":pid,"address":address.to_string(),"hello":hello}));
                let mut requests = vec![];
                if !restart {
                    requests.push(json!({"id":"invalid","method":"connections.create","params":{"profile":{"id":"bad"}}}));
                    requests.push(json!({"id":"create","method":"connections.create","params":{"profile":profile}}));
                }
                requests.extend([
                    json!({"id":"disconnected","method":"ping","connection_id":"qa-native"}),
                    json!({"id":"connect","method":"connections.connect","params":{"id":"qa-native"}}),
                    json!({"id":"list","method":"connections.list"}),
                ]);
                for request in requests {
                    socket.send(tokio_tungstenite::tungstenite::Message::Text(request.to_string().into())).await?;
                    let reply: Value = serde_json::from_str(socket.next().await.ok_or("RPC reply")??.to_text()?)?;
                    transcript.push(json!({"restart":restart,"request":request,"response":reply}));
                    assert_eq!(reply["id"], request["id"]);
                    match request["id"].as_str().ok_or("request id")? {
                        "invalid" | "disconnected" => {
                            assert!(reply.get("error").is_some(), "{reply}");
                            if request["id"] == "invalid" { assert!(!registry.exists()); }
                        }
                        "create" => {
                            assert_eq!(reply["result"]["id"], "qa-native", "{reply}");
                            assert_eq!(reply["result"]["state"], "disconnected", "{reply}");
                            // RED discriminator: current host writes to HOME instead of SHPRD_CONFIG_DIR.
                            let bytes = std::fs::read(&registry).map_err(|error| format!("profile not persisted under SHPRD_CONFIG_DIR: {error}"))?;
                            let persisted: Value = serde_json::from_slice(&bytes)?;
                            assert_eq!(persisted, json!({"version":2,"default_connection_id":"qa-native","profiles":[profile]}));
                            assert_eq!(std::fs::metadata(&registry)?.permissions().mode() & 0o777, 0o600);
                            assert_eq!(std::fs::metadata(&config)?.permissions().mode() & 0o777, 0o700);
                            transcript.push(json!({"registry":registry,"persisted":persisted}));
                        }
                        "connect" => {
                            assert_eq!(reply["result"]["state"], "ready", "{reply}");
                            assert!(reply["result"]["generation"].as_u64().ok_or("generation")? > 0);
                        }
                        "list" => {
                            let item = reply["result"]["connections"].as_array().ok_or("connections")?.iter().find(|item| item["id"] == "qa-native").ok_or("saved profile missing")?;
                            assert_eq!(item["state"], "ready", "{reply}");
                            assert_eq!(item["read_only"], false);
                            assert_eq!(item["control_socket_path"], profile["control_socket_path"]);
                            assert_eq!(item["client_socket_path"], profile["client_socket_path"]);
                            assert_eq!(serde_json::from_slice::<Value>(&std::fs::read(&registry)?)?["profiles"][0], profile);
                        }
                        _ => unreachable!(),
                    }
                }
                subscriptions.recv().await.ok_or("event subscription missing")?;
                socket.close(None).await?;
                Ok::<_, Box<dyn std::error::Error>>(())
            }).await;
            process.kill().await?;
            let exit = process.wait().await?;
            processes.push(json!({"pid":pid,"reaped":true,"exit":exit.to_string()}));
            outcome??;
        }
        while let Some(joined) = tokio::time::timeout(Duration::from_secs(5), fixtures.join_next()).await? {
            joined?.map_err(|error| error.to_string())?;
        }
        assert!(!root.join(".config/herdr-gui/connections.json").exists());
        assert!(!root.join("unused-config").exists());
        Ok::<_, Box<dyn std::error::Error>>(())
    }.await;
    fixtures.shutdown().await;
    directory.close()?;
    println!(
        "C001_CONNECTIONS_WS={}",
        json!({"binary":env!("CARGO_BIN_EXE_shprd"),"transcript":transcript})
    );
    println!(
        "C001_CLEANUP={}",
        json!({"processes":processes,"fixtures_joined_or_cancelled":true,"temporary_directory":root,"temporary_directory_absent":!root.exists(),"control_socket_absent":!control.exists(),"render_socket_absent":!render.exists()})
    );
    result
}

#[cfg(unix)]
#[tokio::test]
async fn stale_generation_and_invalid_profiles() -> Result<(), Box<dyn std::error::Error>> {
    use axum::{body::Body, http::Request};
    use shprd_connections::{ConnectionId, Manager, Profile, ProfileService, Store};
    use shprd_host::{auth::Auth, connections, host};
    use std::{os::unix::fs::PermissionsExt, sync::Arc};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, UnixListener},
        task::JoinSet,
    };
    use tower::ServiceExt;
    // Given two real Herdr wire fixtures and a native profile-backed router.
    let directory = tempfile::tempdir()?;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    let control = directory.path().join("control.sock");
    let render = directory.path().join("render.sock");
    let control_listener = UnixListener::bind(&control)?;
    let render_listener = UnixListener::bind(&render)?;
    let mut fixtures = JoinSet::new();
    fixtures.spawn(async move {
        let mut held = Vec::new();
        for _ in 0..8 {
            let (stream, _) = control_listener.accept().await?;
            let mut stream = BufReader::new(stream);
            let mut line = String::new();
            stream.read_line(&mut line).await?;
            let request: Value = serde_json::from_str(&line)?;
            stream
                .get_mut()
                .write_all(
                    format!(
                        "{}\n",
                        json!({"id":request["id"],"result":{"version":"fixture","protocol":22}})
                    )
                    .as_bytes(),
                )
                .await?;
            if request["method"] == "events.subscribe" {
                held.push(stream.into_inner());
            }
        }
        drop(held);
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    });
    fixtures.spawn(async move {
        for _ in 0..3 {
            let (mut stream, _) = render_listener.accept().await?;
            let length = stream.read_u32_le().await?;
            let mut hello = vec![0; usize::try_from(length)?];
            stream.read_exact(&mut hello).await?;
            let welcome = bincode::encode_to_vec(
                (0_u32, 22_u32, 1_u32, None::<String>),
                bincode::config::standard(),
            )?;
            stream.write_u32_le(u32::try_from(welcome.len())?).await?;
            stream.write_all(&welcome).await?;
        }
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    });
    let manager = Arc::new(Manager::new(ConnectionId::parse("legacy-default")?));
    let profiles = ProfileService::load(
        Store::new(directory.path().join("connections.json"))?,
        Profile::legacy(
            control.to_str().ok_or("path")?,
            render.to_str().ok_or("path")?,
        )?,
        true,
        Arc::clone(&manager),
        connections::factory(),
    )?;
    let startup = profiles.start_configured().await;
    for (_, result) in startup {
        result?;
    }
    let router = host::configured_router_with_profiles(
        control.clone(),
        directory.path().to_path_buf(),
        Auth::new(false, String::new())?,
        None,
        profiles,
        Arc::clone(&manager),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = axum::serve(listener, router.clone()).with_graceful_shutdown(async {
        let _ = stopped.await;
    });
    let client = async {
        let (mut socket, _) =
            tokio_tungstenite::connect_async(format!("ws://{address}/ws")).await?;
        socket.next().await.ok_or("hello")??;
        socket.send(tokio_tungstenite::tungstenite::Message::Text(json!({"id":"create","method":"connections.create","params":{"profile":{"id":"qa", "type":"local", "label":"QA", "auto_connect":true, "control_socket_path":control,"client_socket_path":render}}}).to_string().into())).await?;
        let reply: Value = serde_json::from_str(socket.next().await.ok_or("create")??.to_text()?)?;
        assert_eq!(reply["result"]["state"], "ready", "{reply}");
        println!("SURFACE_READY {}", reply["result"]);
        let old_generation = reply["result"]["generation"].as_u64().ok_or("generation")?;
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/connections/qa/herdr-info?connection_generation={old_generation}"
                    ))
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), 200);
        println!(
            "SURFACE_HTTP_READY status={} generation={old_generation}",
            response.status()
        );
        assert_eq!(response.headers()["X-Herdr-Connection-Id"], "qa");
        assert_eq!(
            response.headers()["X-Herdr-Connection-Generation"],
            old_generation.to_string()
        );
        let info: Value =
            serde_json::from_slice(&axum::body::to_bytes(response.into_body(), 1024).await?)?;
        assert_eq!(info, json!({"version":"fixture","protocol":22}));
        // When one ready connection is retired and reconnected, old HTTP identity must fail.
        for (id, method) in [
            ("disconnect", "connections.disconnect"),
            ("connect", "connections.connect"),
        ] {
            socket
                .send(tokio_tungstenite::tungstenite::Message::Text(
                    json!({"id":id,"method":method,"params":{"id":"qa"}})
                        .to_string()
                        .into(),
                ))
                .await?;
            let reply: Value =
                serde_json::from_str(socket.next().await.ok_or("lifecycle")??.to_text()?)?;
            println!("SURFACE_WS_{} {}", method.to_uppercase(), reply["result"]);
            assert!(reply.get("error").is_none(), "{reply}");
        }
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/connections/qa/herdr-info?connection_generation={old_generation}"
                    ))
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), 409);
        println!("SURFACE_HTTP_STALE status=409 generation={old_generation}");
        socket.send(tokio_tungstenite::tungstenite::Message::Text(json!({"id":"old","method":"ping","connection_id":"qa","connection_generation":old_generation}).to_string().into())).await?;
        let old: Value = serde_json::from_str(socket.next().await.ok_or("stale")??.to_text()?)?;
        assert!(old.get("error").is_some());
        for request in [
            json!({"id":"bad-generation","method":"ping","connection_id":"qa","connection_generation":"1"}),
            json!({"id":"unknown","method":"ping","connection_id":"missing"}),
            json!({"id":"invalid-profile","method":"connections.create","params":{"profile":{"id":"bad"}}}),
        ] {
            socket
                .send(tokio_tungstenite::tungstenite::Message::Text(
                    request.to_string().into(),
                ))
                .await?;
            let rejected: Value =
                serde_json::from_str(socket.next().await.ok_or("rejection")??.to_text()?)?;
            assert_eq!(rejected["id"], request["id"]);
            assert!(rejected.get("error").is_some(), "{rejected}");
        }
        socket.close(None).await?;
        drop(socket);
        stop.send(()).map_err(|()| "shutdown")?;
        Ok::<(), Box<dyn std::error::Error>>(())
    };
    let result = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::try_join!(client, async { server.await.map_err(Into::into) })
    })
    .await;
    manager.stop_all().await?;
    fixtures.shutdown().await;
    println!("SURFACE_CLEANUP manager=stopped fixtures=stopped");
    result??;
    Ok(())
}
