use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{process::Stdio, time::Duration};
use tokio::io::{AsyncBufReadExt, BufReader};

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
        socket.close(None).await?;
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    let result = tokio::time::timeout(Duration::from_secs(10), result).await;
    process.kill().await?;
    process.wait().await?;
    result??;
    Ok(())
}

#[tokio::test]
async fn native_binary_persists_created_connection() -> Result<(), Box<dyn std::error::Error>> {
    // Given the actual standalone binary with an isolated profile registry.
    let directory = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    let registry = directory.path().join("connections.json");
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_shprd"));
    for variable in [
        "HOST",
        "PORT",
        "HERDR_GUI_PASSWORD",
        "HERDR_SESSION",
        "HERDR_SOCKET_PATH",
        "HERDR_CLIENT_SOCKET_PATH",
        "OPEN_BROWSER",
        "SHPRD_CONFIG_DIR",
        "SHPRD_AGENT_DIR",
    ] {
        command.env_remove(variable);
    }
    let mut process = command
        .env("HOME", directory.path())
        .env("USERPROFILE", directory.path())
        .env("APPDATA", directory.path())
        .env("HERDR_GUI_CONNECTIONS_PATH", &registry)
        .args(["--host", "127.0.0.1", "--port", "0"])
        .arg("--socket-path")
        .arg(directory.path().join("missing-control.sock"))
        .arg("--client-socket-path")
        .arg(directory.path().join("missing-render.sock"))
        .kill_on_drop(true)
        .stderr(Stdio::piped())
        .spawn()?;
    let result =
        tokio::time::timeout(Duration::from_secs(10), async {
            let mut startup = BufReader::new(process.stderr.take().ok_or("missing stderr")?);
            let mut line = String::new();
            startup.read_line(&mut line).await?;
            let address = line
                .trim()
                .strip_prefix("SHPRD_LISTENING=http://")
                .ok_or("missing listener address")?;
            let (mut socket, _) =
                tokio_tungstenite::connect_async(format!("ws://{address}/ws")).await?;
            socket.next().await.ok_or("missing hello")??;
            let profile = json!({
                "id":"qa-native", "label":"QA native", "type":"local", "auto_connect":false,
                "control_socket_path":directory.path().join("qa-control.sock"),
                "client_socket_path":directory.path().join("qa-render.sock")
            });
            // When the browser creates a profile, then the host persists it without connecting it.
            socket.send(tokio_tungstenite::tungstenite::Message::Text(
            json!({"id":"create", "method":"connections.create", "params":{"profile":profile}})
                .to_string().into()
        )).await?;
            let reply: Value = loop {
                let reply: Value = serde_json::from_str(
                    socket
                        .next()
                        .await
                        .ok_or("missing create reply")??
                        .to_text()?,
                )?;
                if reply["id"] == "create" {
                    break reply;
                }
            };
            if reply.get("error").is_some() {
                return Err(format!("connection creation failed: {reply}").into());
            }
            assert_eq!(reply["result"]["id"], "qa-native");
            assert_eq!(reply["result"]["state"], "disconnected");
            let persisted: Value = serde_json::from_slice(&std::fs::read(&registry)?)?;
            assert_eq!(persisted["profiles"][0], profile);
            socket.close(None).await?;
            Ok::<_, Box<dyn std::error::Error>>(())
        })
        .await;
    process.kill().await?;
    process.wait().await?;
    result?
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
        for _ in 0..4 {
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
        }
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
    result??;
    Ok(())
}
