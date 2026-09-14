use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use shprd_host::{auth::Auth, host};
use tokio::net::TcpListener;

type ScrollWire = (u32, u32, u32, u16, Option<u16>, Option<u16>, u8);

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
async fn terminal_attach_rejects_out_of_range_surface_geometry()
-> Result<(), Box<dyn std::error::Error>> {
    // Given a live native HTTP/WebSocket host without downstream terminal state.
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
        let (mut socket, _) =
            tokio_tungstenite::connect_async(format!("ws://{address}/ws")).await?;
        socket.next().await.ok_or("missing hello")??;
        socket
            .send(tokio_tungstenite::tungstenite::Message::Text(
                json!({
                    "id":"attach-bad",
                    "method":"terminal.attach",
                    "params":{"terminal_id":"term-1","surface_cols":65536,"surface_rows":24}
                })
                .to_string()
                .into(),
            ))
            .await?;
        let response: Value = serde_json::from_str(
            socket
                .next()
                .await
                .ok_or("missing terminal attach rejection")??
                .to_text()?,
        )?;
        // When an invalid geometry crosses the real socket, then boundary validation reports it.
        assert_eq!(response["id"], "attach-bad");
        assert_eq!(
            response["error"]["message"],
            "surface_cols and surface_rows must be integers between 1 and 65535"
        );
        socket.close(None).await?;
        stop.send(()).map_err(|()| "shutdown lost")?;
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::try_join!(client, async { server.await.map_err(Into::into) })
    })
    .await??;
    Ok(())
}

#[tokio::test]
async fn terminal_websocket_c3_rejects_boundary_actions_without_state_leak()
-> Result<(), Box<dyn std::error::Error>> {
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
            .map_err(|error| -> Box<dyn std::error::Error> { Box::new(error) })
    };
    let client = async {
        let (mut socket, _) =
            tokio_tungstenite::connect_async(format!("ws://{address}/ws")).await?;
        socket.next().await.ok_or("missing websocket hello")??;
        let cases = [
            (
                "resize-zero",
                "terminal.resize",
                json!({"terminal_id":"term-c3","cols":0,"rows":24}),
                "valid terminal cols and rows required",
            ),
            (
                "resize-high",
                "terminal.resize",
                json!({"terminal_id":"term-c3","cols":65536,"rows":24}),
                "valid terminal cols and rows required",
            ),
            (
                "resize-malformed",
                "terminal.resize",
                json!({"terminal_id":"term-c3","cols":"80","rows":24}),
                "invalid cols",
            ),
            (
                "attach-high",
                "terminal.attach",
                json!({"terminal_id":"term-c3","cols":65536,"rows":24}),
                "valid terminal cols and rows required",
            ),
            (
                "scroll-zero",
                "terminal.scroll",
                json!({"terminal_id":"term-c3","direction":"up","lines":0}),
                "terminal scroll requires positive integer lines",
            ),
            (
                "input-malformed",
                "terminal.input",
                json!({"terminal_id":"term-c3","data":"%%%"}),
                "invalid terminal input",
            ),
        ];
        for (id, method, params, error_message) in cases {
            socket
                .send(tokio_tungstenite::tungstenite::Message::Text(
                    json!({"id":id,"method":method,"params":params})
                        .to_string()
                        .into(),
                ))
                .await?;
            let response: Value = serde_json::from_str(
                socket
                    .next()
                    .await
                    .ok_or("missing boundary response")??
                    .to_text()?,
            )?;
            assert_eq!(response["id"], id);
            assert_eq!(response["error"]["message"], error_message);
            assert_eq!(response["connection_id"], "legacy-default");
            assert_eq!(response["connection_generation"], 1);
            assert!(response.get("result").is_none());
        }

        socket
            .send(tokio_tungstenite::tungstenite::Message::Text(
                json!({"id":"detach-unknown","method":"terminal.detach","params":{"terminal_id":"term-c3"}})
                    .to_string()
                    .into(),
            ))
            .await?;
        let detached: Value = serde_json::from_str(
            socket
                .next()
                .await
                .ok_or("missing idempotent detach response")??
                .to_text()?,
        )?;
        assert_eq!(detached["id"], "detach-unknown");
        assert_eq!(detached["result"], json!({"ok":true}));
        socket.close(None).await?;
        stop.send(()).map_err(|()| "shutdown lost")?;
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::try_join!(client, server)
    })
    .await??;
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn terminal_websocket_endpoint_c1_sequence_focuses_crops_and_encodes_input()
-> Result<(), Box<dyn std::error::Error>> {
    use shprd_connections::{
        ConnectionId, Manager, Profile, ProfileService, Runtime, RuntimeContext, RuntimeFuture,
        SocketPaths, Store,
    };
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::net::UnixListener;
    use tokio_util::codec::LengthDelimitedCodec;

    struct Ready(PathBuf);
    impl Runtime for Ready {
        fn start<'a>(&'a self, _: &'a RuntimeContext) -> RuntimeFuture<'a, SocketPaths> {
            let render = self.0.clone();
            Box::pin(async move {
                Ok(SocketPaths {
                    control: "/fixture/control".into(),
                    render,
                })
            })
        }
        fn stop(&self) -> RuntimeFuture<'_, ()> {
            Box::pin(async { Ok(()) })
        }
    }

    let directory = tempfile::tempdir()?;
    let render_path = directory.path().join("endpoint.sock");
    let render_listener = UnixListener::bind(&render_path)?;
    let fixture = tokio::spawn(async move {
        let (socket, _) = render_listener.accept().await?;
        let mut wire = LengthDelimitedCodec::builder()
            .little_endian()
            .new_framed(socket);
        let hello = wire.next().await.ok_or("missing endpoint hello")??;
        let ((tag, kind, data), _): ((u32, String, String), usize) =
            bincode::decode_from_slice(&hello, bincode::config::standard())?;
        assert_eq!((tag, kind.as_str()), (20, "endpoint.hello.v1"));
        let hello: Value = serde_json::from_str(&data)?;
        assert_eq!(hello["generation"], 1);
        assert_eq!(hello["surface_size"], json!({"cols":100,"rows":30}));

        let welcome = json!({
            "generation":1,
            "server_version":"fixture",
            "snapshot_codec":"shell.snapshot.v1",
            "surface_codec":"shell.surface.v1",
            "input_codec":"shell.input.semantic.v1",
            "blob_codec":"shell.blob.v1",
            "methods":["pane.focus","pane.scroll"],
            "capabilities":[]
        });
        for (kind, data) in [
            ("endpoint.welcome.v1", welcome),
            (
                "shell.snapshot.v1",
                json!({"boot_id":"boot-c1","revision":1}),
            ),
        ] {
            wire.send(
                bincode::encode_to_vec(
                    (20_u32, kind, data.to_string()),
                    bincode::config::standard(),
                )?
                .into(),
            )
            .await?;
        }

        let focus = wire.next().await.ok_or("missing pane.focus")??;
        let ((tag, boot, data), _): ((u32, String, String), usize) =
            bincode::decode_from_slice(&focus, bincode::config::standard())?;
        assert_eq!((tag, boot.as_str()), (15, "boot-c1"));
        assert_eq!(
            serde_json::from_str::<Value>(&data)?["method"],
            "pane.focus"
        );

        let cell = ("x".to_owned(), 0_u32, 0_u32, 0_u16, false, None::<u32>);
        let cells = vec![cell; 100 * 30];
        let frame = (
            cells,
            100_u16,
            30_u16,
            Some((1_u16, 1_u16, true, 1_u8)),
            Vec::<String>::new(),
            Vec::<u8>::new(),
        );
        let pane = (
            "pane-1",
            1_u64,
            (0_u16, 0_u16, 100_u16, 30_u16),
            (0_u16, 0_u16, 80_u16, 24_u16),
            None::<(u16, u16, u16, u16)>,
            Some((0_u64, 10_u64, 24_u16)),
            true,
            true,
            false,
            false,
            0_u16,
            0_u16,
        );
        let surface = bincode::encode_to_vec(
            ("boot-c1", 1_u64, 1_u64, frame, vec![pane]),
            bincode::config::standard(),
        )?;
        let mut surface_message = bincode::encode_to_vec(13_u32, bincode::config::standard())?;
        surface_message.extend_from_slice(&surface);
        wire.send(surface_message.into()).await?;
        for (last, chunk) in [
            (false, &b"{\"result\":"[..]),
            (true, &b"{\"focused\":true}}"[..]),
        ] {
            wire.send(
                bincode::encode_to_vec(
                    (18_u32, "boot-c1", "focus-1", last, chunk),
                    bincode::config::standard(),
                )?
                .into(),
            )
            .await?;
        }

        let input = wire.next().await.ok_or("missing semantic input")??;
        let ((tag, pane_id, count), _): ((u32, String, usize), usize) =
            bincode::decode_from_slice(&input, bincode::config::standard())?;
        assert_eq!((tag, pane_id.as_str(), count), (13, "pane-1", 2));

        let resize = wire.next().await.ok_or("missing endpoint resize")??;
        let ((tag, x, y, cols, rows, pixels), _): ((u32, u16, u16, u16, u16, bool), usize) =
            bincode::decode_from_slice(&resize, bincode::config::standard())?;
        assert_eq!((tag, x, y, cols, rows, pixels), (12, 0, 0, 100, 30, false));

        let scroll = wire.next().await.ok_or("missing pane.scroll")??;
        let ((tag, boot, data), _): ((u32, String, String), usize) =
            bincode::decode_from_slice(&scroll, bincode::config::standard())?;
        assert_eq!((tag, boot.as_str()), (15, "boot-c1"));
        let scroll: Value = serde_json::from_str(&data)?;
        assert_eq!(scroll["method"], "pane.scroll");
        assert_eq!(scroll["params"]["offset_from_bottom"], 3);
        while wire.next().await.transpose()?.is_some() {}
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    });

    let id = ConnectionId::parse("legacy-default")?;
    let manager = Arc::new(Manager::new(id.clone()));
    let render_for_runtime = render_path.clone();
    let profiles = ProfileService::load(
        Store::new(directory.path().join("connections.json"))?,
        Profile::legacy(
            "/fixture/control",
            render_path.to_str().ok_or("non-utf8 render path")?,
        )?,
        true,
        Arc::clone(&manager),
        Arc::new(move |_| {
            let render = render_for_runtime.clone();
            Arc::new(move |_| Ok(Arc::new(Ready(render.clone()))))
        }),
    )?;
    manager.connect(&id).await?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let router = host::configured_router_with_profiles(
        "/fixture/control".into(),
        directory.path().to_path_buf(),
        Auth::new(false, String::new())?,
        None,
        profiles,
        Arc::clone(&manager),
    );
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = async {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .map_err(|error| -> Box<dyn std::error::Error> { Box::new(error) })
    };
    let client = async {
        let (mut socket, _) =
            tokio_tungstenite::connect_async(format!("ws://{address}/ws")).await?;
        socket.next().await.ok_or("missing websocket hello")??;
        let send = |id: &str, method: &str, params: Value| json!({"id":id,"method":method,"connection_id":"legacy-default","connection_generation":1,"params":params});
        let mut pending_frame = None;
        for (id, method, params) in [
            (
                "attach",
                "terminal.attach",
                json!({"terminal_id":"term-c1","pane_id":"pane-1","cols":80,"rows":24,"surface_cols":100,"surface_rows":30}),
            ),
            (
                "input",
                "terminal.input",
                json!({"terminal_id":"term-c1","data":"cHdkDQ=="}),
            ),
            (
                "resize",
                "terminal.resize",
                json!({"terminal_id":"term-c1","cols":100,"rows":30}),
            ),
            (
                "scroll",
                "terminal.scroll",
                json!({"terminal_id":"term-c1","direction":"up","lines":3,"column":4,"row":5,"source":"wheel"}),
            ),
        ] {
            socket
                .send(tokio_tungstenite::tungstenite::Message::Text(
                    send(id, method, params).to_string().into(),
                ))
                .await?;
            let response = loop {
                let value: Value = serde_json::from_str(
                    socket
                        .next()
                        .await
                        .ok_or("missing endpoint response")??
                        .to_text()?,
                )?;
                if value.get("terminal").is_some() {
                    pending_frame = Some(value);
                    continue;
                }
                if value["id"] == id {
                    break value;
                }
            };
            assert_eq!(response["connection_id"], "legacy-default");
            assert_eq!(response["connection_generation"], 1);
            if response["result"]["ok"] != true {
                return Err(format!("endpoint response mismatch: {response}").into());
            }
        }
        let frame = if let Some(value) = pending_frame {
            value
        } else {
            loop {
                let value: Value = serde_json::from_str(
                    socket
                        .next()
                        .await
                        .ok_or("missing endpoint frame")??
                        .to_text()?,
                )?;
                if value.get("terminal").is_some() {
                    break value;
                }
            }
        };
        assert_eq!(frame["terminal"]["width"], 80);
        assert_eq!(frame["terminal"]["height"], 24);
        assert!(!frame["terminal"]["bytes"].as_str().unwrap_or("").is_empty());
        socket
            .send(tokio_tungstenite::tungstenite::Message::Text(
                send(
                    "detach",
                    "terminal.detach",
                    json!({"terminal_id":"term-c1"}),
                )
                .to_string()
                .into(),
            ))
            .await?;
        let response = loop {
            let value: Value = serde_json::from_str(
                socket
                    .next()
                    .await
                    .ok_or("missing endpoint detach")??
                    .to_text()?,
            )?;
            if value["id"] == "detach" {
                break value;
            }
        };
        assert_eq!(response["id"], "detach");
        assert_eq!(response["result"], json!({"ok":true}));
        socket.close(None).await?;
        stop.send(()).map_err(|()| "shutdown lost")?;
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::try_join!(client, server)
    })
    .await??;
    fixture
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn terminal_websocket_c2_two_viewer_crop_clipboard_and_generation_isolation()
-> Result<(), Box<dyn std::error::Error>> {
    use tokio::net::UnixListener;
    use tokio_util::codec::LengthDelimitedCodec;

    let directory = tempfile::tempdir()?;
    let render_path = directory.path().join("render.sock");
    let render_listener = UnixListener::bind(&render_path)?;
    let fixture = tokio::spawn(async move {
        let (socket, _) = render_listener.accept().await?;
        let mut wire = LengthDelimitedCodec::builder()
            .little_endian()
            .new_framed(socket);
        assert_eq!(
            wire.next()
                .await
                .ok_or("missing generation-one hello")??
                .as_ref(),
            &[0, 20, 80, 24, 0, 0, 1, 0, 2]
        );
        wire.send(vec![0, 20, 1, 0].into()).await?;
        let attach = wire.next().await.ok_or("missing generation-one attach")??;
        let ((tag, terminal_id, takeover), _): ((u32, String, bool), usize) =
            bincode::decode_from_slice(&attach, bincode::config::standard())?;
        assert_eq!((tag, terminal_id.as_str(), takeover), (5, "term-c2", true));

        let mut input_count = 0_u64;
        while let Some(message) = wire.next().await {
            let message = message?;
            let (tag, _): (u32, usize) =
                bincode::decode_from_slice(&message, bincode::config::standard())?;
            if tag != 1 {
                continue;
            }
            input_count += 1;
            wire.send(
                bincode::encode_to_vec(
                    (
                        2_u32,
                        input_count,
                        120_u16,
                        40_u16,
                        true,
                        if input_count == 1 {
                            b"frame-a".to_vec()
                        } else {
                            b"frame-b".to_vec()
                        },
                    ),
                    bincode::config::standard(),
                )?
                .into(),
            )
            .await?;
            if input_count == 2 {
                wire.send(
                    bincode::encode_to_vec((6_u32, "clipboard-b"), bincode::config::standard())?
                        .into(),
                )
                .await?;
            }
        }

        let (socket, _) = render_listener.accept().await?;
        let mut wire = LengthDelimitedCodec::builder()
            .little_endian()
            .new_framed(socket);
        assert_eq!(
            wire.next()
                .await
                .ok_or("missing generation-two hello")??
                .as_ref(),
            &[0, 20, 120, 40, 0, 0, 1, 0, 2]
        );
        wire.send(vec![0, 20, 1, 0].into()).await?;
        let attach = wire.next().await.ok_or("missing generation-two attach")??;
        let ((tag, terminal_id, takeover), _): ((u32, String, bool), usize) =
            bincode::decode_from_slice(&attach, bincode::config::standard())?;
        assert_eq!((tag, terminal_id.as_str(), takeover), (5, "term-c2", true));
        wire.send(
            bincode::encode_to_vec(
                (
                    2_u32,
                    1_u64,
                    120_u16,
                    40_u16,
                    true,
                    b"new-generation".to_vec(),
                ),
                bincode::config::standard(),
            )?
            .into(),
        )
        .await?;
        while wire.next().await.transpose()?.is_some() {}
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    });

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let router = host::configured_router(
        render_path,
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
            .map_err(|error| -> Box<dyn std::error::Error> { Box::new(error) })
    };
    let client = async {
        let (mut viewer_a, _) =
            tokio_tungstenite::connect_async(format!("ws://{address}/ws")).await?;
        viewer_a.next().await.ok_or("missing viewer A hello")??;
        viewer_a
            .send(tokio_tungstenite::tungstenite::Message::Text(
                json!({"id":"attach-a","method":"terminal.attach","params":{"terminal_id":"term-c2","cols":80,"rows":24}})
                    .to_string()
                    .into(),
            ))
            .await?;
        let attach_a: Value = serde_json::from_str(
            viewer_a
                .next()
                .await
                .ok_or("missing viewer A attach")??
                .to_text()?,
        )?;
        assert_eq!(attach_a["result"], json!({"ok":true}));

        let (mut viewer_b, _) =
            tokio_tungstenite::connect_async(format!("ws://{address}/ws")).await?;
        viewer_b.next().await.ok_or("missing viewer B hello")??;
        viewer_b
            .send(tokio_tungstenite::tungstenite::Message::Text(
                json!({"id":"attach-b","method":"terminal.attach","params":{"terminal_id":"term-c2","cols":120,"rows":40}})
                    .to_string()
                    .into(),
            ))
            .await?;
        let attach_b: Value = serde_json::from_str(
            viewer_b
                .next()
                .await
                .ok_or("missing viewer B attach")??
                .to_text()?,
        )?;
        assert_eq!(attach_b["result"], json!({"ok":true}));

        viewer_a
            .send(tokio_tungstenite::tungstenite::Message::Text(
                json!({"id":"input-a","method":"terminal.input","params":{"terminal_id":"term-c2","data":"YQ=="}})
                    .to_string()
                    .into(),
            ))
            .await?;
        let mut response_a = false;
        let mut frame_a = None;
        while !response_a || frame_a.is_none() {
            let value: Value = serde_json::from_str(
                viewer_a
                    .next()
                    .await
                    .ok_or("missing viewer A first event")??
                    .to_text()?,
            )?;
            if value["id"] == "input-a" {
                assert_eq!(value["result"], json!({"ok":true}));
                response_a = true;
            }
            assert!(value.get("terminal_clipboard").is_none());
            if value.get("terminal").is_some() {
                frame_a = Some(value);
            }
        }
        let frame_a = frame_a.ok_or("viewer A first frame")?;
        assert_eq!(frame_a["terminal"]["width"], 120);
        assert_eq!(frame_a["terminal"]["height"], 40);

        let mut frame_b = None;
        while frame_b.is_none() {
            let value: Value = serde_json::from_str(
                viewer_b
                    .next()
                    .await
                    .ok_or("missing viewer B first frame")??
                    .to_text()?,
            )?;
            assert!(value.get("terminal_clipboard").is_none());
            if value.get("terminal").is_some() {
                frame_b = Some(value);
            }
        }
        assert_eq!(
            frame_b.ok_or("viewer B first frame")?["terminal"]["width"],
            120
        );

        viewer_b
            .send(tokio_tungstenite::tungstenite::Message::Text(
                json!({"id":"input-b","method":"terminal.input","params":{"terminal_id":"term-c2","data":"Yg=="}})
                    .to_string()
                    .into(),
            ))
            .await?;
        let mut response_b = false;
        let mut frame_b2 = None;
        let mut clipboard_b = None;
        while !response_b || frame_b2.is_none() || clipboard_b.is_none() {
            let value: Value = serde_json::from_str(
                viewer_b
                    .next()
                    .await
                    .ok_or("missing viewer B second event")??
                    .to_text()?,
            )?;
            if value["id"] == "input-b" {
                assert_eq!(value["result"], json!({"ok":true}));
                response_b = true;
            }
            let is_frame = value.get("terminal").is_some();
            let is_clipboard = value.get("terminal_clipboard").is_some();
            if is_frame {
                frame_b2 = Some(value.clone());
            }
            if is_clipboard {
                clipboard_b = Some(value);
            }
        }
        assert_eq!(
            frame_b2.ok_or("viewer B second frame")?["terminal"]["width"],
            120
        );
        assert_eq!(
            clipboard_b.ok_or("viewer B clipboard")?["terminal_clipboard"]["data"],
            "clipboard-b"
        );

        let mut frame_a2 = None;
        while frame_a2.is_none() {
            let value: Value = serde_json::from_str(
                viewer_a
                    .next()
                    .await
                    .ok_or("missing viewer A second frame")??
                    .to_text()?,
            )?;
            assert!(value.get("terminal_clipboard").is_none());
            if value.get("terminal").is_some() {
                frame_a2 = Some(value);
            }
        }
        assert_eq!(
            frame_a2.ok_or("viewer A second frame")?["terminal"]["width"],
            120
        );

        for (socket, id) in [(&mut viewer_a, "detach-a"), (&mut viewer_b, "detach-b")] {
            socket
                .send(tokio_tungstenite::tungstenite::Message::Text(
                    json!({"id":id,"method":"terminal.detach","params":{"terminal_id":"term-c2"}})
                        .to_string()
                        .into(),
                ))
                .await?;
            let response: Value = serde_json::from_str(
                socket
                    .next()
                    .await
                    .ok_or("missing viewer detach")??
                    .to_text()?,
            )?;
            assert_eq!(response["result"], json!({"ok":true}));
        }
        viewer_a.close(None).await?;
        viewer_b.close(None).await?;

        let (mut viewer_c, _) =
            tokio_tungstenite::connect_async(format!("ws://{address}/ws")).await?;
        viewer_c
            .next()
            .await
            .ok_or("missing generation-two hello")??;
        viewer_c
            .send(tokio_tungstenite::tungstenite::Message::Text(
                json!({"id":"attach-c","method":"terminal.attach","params":{"terminal_id":"term-c2","cols":120,"rows":40}})
                    .to_string()
                    .into(),
            ))
            .await?;
        let attach_c: Value = serde_json::from_str(
            viewer_c
                .next()
                .await
                .ok_or("missing generation-two attach response")??
                .to_text()?,
        )?;
        assert_eq!(attach_c["result"], json!({"ok":true}));
        let frame_c: Value = serde_json::from_str(
            viewer_c
                .next()
                .await
                .ok_or("missing generation-two frame")??
                .to_text()?,
        )?;
        assert_eq!(frame_c["terminal"]["width"], 120);
        assert_eq!(frame_c["terminal"]["bytes"], "bmV3LWdlbmVyYXRpb24=");
        viewer_c.close(None).await?;
        stop.send(()).map_err(|()| "shutdown lost")?;
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    tokio::time::timeout(std::time::Duration::from_secs(8), async {
        tokio::try_join!(client, server)
    })
    .await??;
    fixture
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn terminal_websocket_c1_sequence_preserves_envelopes_and_wire_actions()
-> Result<(), Box<dyn std::error::Error>> {
    use tokio::net::UnixListener;
    use tokio_util::codec::LengthDelimitedCodec;

    let directory = tempfile::tempdir()?;
    let render_path = directory.path().join("render.sock");
    let render_listener = UnixListener::bind(&render_path)?;
    let fixture = tokio::spawn(async move {
        let (socket, _) = render_listener.accept().await?;
        let mut wire = LengthDelimitedCodec::builder()
            .little_endian()
            .new_framed(socket);
        assert_eq!(
            wire.next().await.ok_or("missing terminal hello")??.as_ref(),
            &[0, 20, 80, 24, 0, 0, 1, 0, 2]
        );
        wire.send(vec![0, 20, 1, 0].into()).await?;
        let attach = wire.next().await.ok_or("missing terminal attach")??;
        let ((tag, terminal_id, takeover), _): ((u32, String, bool), usize) =
            bincode::decode_from_slice(&attach, bincode::config::standard())?;
        assert_eq!((tag, terminal_id.as_str(), takeover), (5, "term-c1", true));

        let input = wire.next().await.ok_or("missing terminal input")??;
        let ((tag, bytes), _): ((u32, Vec<u8>), usize) =
            bincode::decode_from_slice(&input, bincode::config::standard())?;
        assert_eq!((tag, bytes), (1, b"pwd\r".to_vec()));

        let resize = wire.next().await.ok_or("missing terminal resize")??;
        let ((tag, cols, rows, x, y), _): ((u32, u16, u16, u16, u16), usize) =
            bincode::decode_from_slice(&resize, bincode::config::standard())?;
        assert_eq!((tag, cols, rows, x, y), (3, 100, 30, 0, 0));

        let scroll = wire.next().await.ok_or("missing terminal scroll")??;
        let ((tag, source, direction, lines, column, row, flags), _): (ScrollWire, usize) =
            bincode::decode_from_slice(&scroll, bincode::config::standard())?;
        assert_eq!(
            (tag, source, direction, lines, column, row, flags),
            (6, 0, 0, 3, Some(4), Some(5), 0)
        );
        wire.send(
            bincode::encode_to_vec(
                (2_u32, 1_u64, 100_u16, 30_u16, true, b"c1-frame".to_vec()),
                bincode::config::standard(),
            )?
            .into(),
        )
        .await?;
        while wire.next().await.transpose()?.is_some() {}
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    });

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let router = host::configured_router(
        render_path,
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
            .map_err(|error| -> Box<dyn std::error::Error> { Box::new(error) })
    };
    let client = async {
        let (mut socket, _) =
            tokio_tungstenite::connect_async(format!("ws://{address}/ws")).await?;
        let hello: Value = serde_json::from_str(
            socket
                .next()
                .await
                .ok_or("missing websocket hello")??
                .to_text()?,
        )?;
        assert_eq!(hello["bridge_protocol_version"], 2);

        for (id, method, params) in [
            (
                "attach",
                "terminal.attach",
                json!({"terminal_id":"term-c1","cols":80,"rows":24}),
            ),
            (
                "input",
                "terminal.input",
                json!({"terminal_id":"term-c1","data":"cHdkDQ=="}),
            ),
            (
                "resize",
                "terminal.resize",
                json!({"terminal_id":"term-c1","cols":100,"rows":30}),
            ),
            (
                "scroll",
                "terminal.scroll",
                json!({"terminal_id":"term-c1","direction":"up","lines":3,"column":4,"row":5,"source":"wheel"}),
            ),
        ] {
            socket
                .send(tokio_tungstenite::tungstenite::Message::Text(
                    json!({"id":id,"method":method,"params":params})
                        .to_string()
                        .into(),
                ))
                .await?;
            let response: Value = serde_json::from_str(
                socket
                    .next()
                    .await
                    .ok_or("missing terminal response")??
                    .to_text()?,
            )?;
            assert_eq!(response["id"], id);
            assert_eq!(response["result"], json!({"ok":true}));
            assert_eq!(response["connection_id"], "legacy-default");
            assert_eq!(response["connection_generation"], 1);
        }

        let event: Value = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                let message = socket.next().await.ok_or("missing terminal frame")??;
                if message.is_text() {
                    let value: Value = serde_json::from_str(message.to_text()?)?;
                    if value.get("terminal").is_some() {
                        break Ok::<_, Box<dyn std::error::Error>>(value);
                    }
                }
            }
        })
        .await??;
        assert_eq!(event["terminal"]["terminal_id"], "term-c1");
        assert_eq!(event["terminal"]["width"], 100);
        assert_eq!(event["terminal"]["height"], 30);
        assert_eq!(event["terminal"]["bytes"], "YzEtZnJhbWU=");
        assert_eq!(event["connection_id"], "legacy-default");
        assert_eq!(event["connection_generation"], 1);

        socket
            .send(tokio_tungstenite::tungstenite::Message::Text(
                json!({"id":"detach","method":"terminal.detach","params":{"terminal_id":"term-c1"}})
                    .to_string()
                    .into(),
            ))
            .await?;
        let response: Value = serde_json::from_str(
            socket
                .next()
                .await
                .ok_or("missing terminal detach response")??
                .to_text()?,
        )?;
        assert_eq!(response["id"], "detach");
        assert_eq!(response["result"], json!({"ok":true}));
        socket.close(None).await?;
        stop.send(()).map_err(|()| "shutdown lost")?;
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::try_join!(client, server)
    })
    .await??;
    fixture
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;
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
