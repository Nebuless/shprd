#![cfg(unix)]
use futures_util::{SinkExt, StreamExt};
use shprd_host::terminal::{Output, Terminal};
use tokio::net::UnixListener;
use tokio_util::codec::LengthDelimitedCodec;

type ScrollWire = (u32, u32, u32, u16, Option<u16>, Option<u16>, u8);

#[tokio::test]
async fn tagged_socket_contract_preserves_attach_input_and_output()
-> Result<(), Box<dyn std::error::Error>> {
    for protocol in [20, 22] {
        // Given a real render socket with hand-transcribed tagged wire frames.
        let home = tempfile::tempdir()?;
        let path = home.path().join("render.sock");
        let listener = UnixListener::bind(&path)?;
        let server = async {
            let (socket, _) = listener.accept().await?;
            let mut wire = LengthDelimitedCodec::builder()
                .little_endian()
                .new_framed(socket);
            let handshake = wire.next().await.ok_or("missing hello")??;
            assert_eq!(
                handshake.as_ref(),
                if protocol == 22 {
                    &[0, 22, 100, 30, 0, 0, 0][..]
                } else {
                    &[0, 20, 100, 30, 0, 0, 1, 0, 2][..]
                }
            );
            wire.send(vec![0, protocol, 1, 0].into()).await?;
            let attach = wire.next().await.ok_or("missing attach")??;
            assert_eq!(attach.as_ref(), b"\x05\x06term_1\x00");
            let mut frame = vec![if protocol == 22 { 1 } else { 2 }, 1, 100, 30, 1, 5];
            frame.extend_from_slice(b"hello");
            wire.send(frame.into()).await?;
            let input = wire.next().await.ok_or("missing input")??;
            assert_eq!(input.as_ref(), b"\x01\x03abc");
            wire.send(vec![if protocol == 22 { 3 } else { 4 }, 0].into())
                .await?;
            Ok::<_, Box<dyn std::error::Error>>(())
        };
        let client = async {
            // When the client attaches, then output and input keep the frozen protocol.
            let mut terminal = Terminal::connect(&path, u32::from(protocol), 100, 30).await?;
            terminal.attach("term_1", false).await?;
            let Output::Terminal {
                width,
                height,
                bytes,
                ..
            } = terminal.next().await?
            else {
                return Err("unexpected terminal output".into());
            };
            assert_eq!((width, height, bytes), (100, 30, b"hello".to_vec()));
            terminal.input(b"abc").await?;
            assert!(matches!(terminal.next().await?, Output::Closed(_)));
            Ok::<_, Box<dyn std::error::Error>>(())
        };
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            tokio::try_join!(client, server)
        })
        .await??;
    }
    Ok(())
}

#[tokio::test]
async fn terminal_bridge_happy_path() -> Result<(), Box<dyn std::error::Error>> {
    use shprd_host::terminal_bridge::{
        AttachRequest, ScrollRequest, TerminalBridge, TerminalEvent, Transport,
    };
    use tokio::sync::mpsc;

    let home = tempfile::tempdir()?;
    let path = home.path().join("render.sock");
    let listener = UnixListener::bind(&path)?;
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await?;
        let mut wire = LengthDelimitedCodec::builder()
            .little_endian()
            .new_framed(socket);
        assert_eq!(
            wire.next().await.ok_or("missing hello")??.as_ref(),
            &[0, 20, 80, 24, 0, 0, 1, 0, 2]
        );
        wire.send(vec![0, 20, 1, 0].into()).await?;
        let attach = wire.next().await.ok_or("missing attach")??;
        let ((tag, terminal_id, takeover), _): ((u32, String, bool), usize) =
            bincode::decode_from_slice(&attach, bincode::config::standard())?;
        assert_eq!((tag, terminal_id.as_str(), takeover), (5, "term-1", true));
        wire.send(
            bincode::encode_to_vec(
                (2_u32, 1_u64, 120_u16, 40_u16, true, b"native".to_vec()),
                bincode::config::standard(),
            )?
            .into(),
        )
        .await?;
        let input = wire.next().await.ok_or("missing input")??;
        let ((tag, bytes), _): ((u32, Vec<u8>), usize) =
            bincode::decode_from_slice(&input, bincode::config::standard())?;
        assert_eq!((tag, bytes), (1, b"pwd\r".to_vec()));
        let resize = wire.next().await.ok_or("missing resize")??;
        let ((tag, cols, rows, x, y), _): ((u32, u16, u16, u16, u16), usize) =
            bincode::decode_from_slice(&resize, bincode::config::standard())?;
        assert_eq!((tag, cols, rows, x, y), (3, 100, 30, 0, 0));
        let scroll = wire.next().await.ok_or("missing scroll")??;
        let ((tag, source, direction, lines, column, row, flags), _) =
            bincode::decode_from_slice::<ScrollWire, _>(&scroll, bincode::config::standard())?;
        assert_eq!(
            (tag, source, direction, lines, column, row, flags),
            (6, 0, 0, 3, Some(4), Some(5), 0)
        );
        while wire.next().await.transpose()?.is_some() {}
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    });
    let bridge = TerminalBridge::new(path);
    let (sender, mut events) = mpsc::unbounded_channel::<TerminalEvent>();
    bridge
        .attach(AttachRequest {
            viewer_id: "attachment-a".to_owned(),
            terminal_id: "term-1".to_owned(),
            cols: 80,
            rows: 24,
            transport: Transport::Direct(20),
            surface_cols: None,
            surface_rows: None,
            sender,
        })
        .await?;
    let frame = tokio::time::timeout(std::time::Duration::from_secs(1), events.recv())
        .await?
        .ok_or("missing terminal frame")?
        .publish()
        .ok_or("stale terminal frame")?;
    assert_eq!(frame["terminal"]["terminal_id"], "term-1");
    assert_eq!(frame["terminal"]["width"], 120);
    assert_eq!(frame["terminal"]["height"], 40);
    assert_eq!(frame["terminal"]["bytes"], "bmF0aXZl");
    assert_eq!(frame["connection_id"], "legacy-default");
    assert_eq!(frame["connection_generation"], 1);
    bridge
        .input("attachment-a", "term-1", "cHdkDQ==", 100)
        .await?;
    bridge.resize("attachment-a", "term-1", 100, 30).await?;
    bridge
        .scroll(ScrollRequest {
            viewer_id: "attachment-a".to_owned(),
            terminal_id: "term-1".to_owned(),
            direction: "up".to_owned(),
            lines: 3,
            column: Some(4),
            row: Some(5),
            source: "wheel".to_owned(),
        })
        .await?;
    bridge.publish_clipboard("term-1", "copied", 200).await;
    assert_eq!(
        events
            .recv()
            .await
            .ok_or("missing clipboard")?
            .publish()
            .ok_or("stale clipboard")?["terminal_clipboard"]["data"],
        "copied"
    );
    bridge.detach("attachment-a", Some("term-1")).await;
    let server_result = tokio::time::timeout(std::time::Duration::from_secs(1), server)
        .await
        .map_err(|error| error.to_string())?;
    let _: () = server_result
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tokio::test]
async fn terminal_bridge_raw_frame_preserves_source_dimensions_and_bytes()
-> Result<(), Box<dyn std::error::Error>> {
    use shprd_host::terminal_bridge::{
        AttachRequest, TerminalBridge, TerminalEvent, Transport, ViewerFrame,
    };
    use tokio::sync::mpsc;

    let directory = tempfile::tempdir()?;
    let path = directory.path().join("render.sock");
    let listener = UnixListener::bind(&path)?;
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await?;
        let mut wire = LengthDelimitedCodec::builder()
            .little_endian()
            .new_framed(socket);
        wire.next().await.ok_or("missing hello")??;
        wire.send(vec![0, 20, 1, 0].into()).await?;
        wire.next().await.ok_or("missing attach")??;
        while wire.next().await.transpose()?.is_some() {}
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    });
    let bridge = TerminalBridge::new(path);
    let (sender, mut events) = mpsc::unbounded_channel::<TerminalEvent>();
    bridge
        .attach(AttachRequest {
            viewer_id: "viewer".into(),
            terminal_id: "term".into(),
            cols: 80,
            rows: 24,
            transport: Transport::Direct(20),
            surface_cols: None,
            surface_rows: None,
            sender,
        })
        .await?;

    bridge
        .publish_frame(ViewerFrame {
            terminal_id: "term".into(),
            width: 120,
            height: 40,
            full: true,
            mouse_reporting: Some(true),
            bytes: b"\x1b[2Jraw source bytes".to_vec(),
        })
        .await;
    let event = events
        .recv()
        .await
        .ok_or("missing raw frame")?
        .publish()
        .ok_or("stale raw frame")?;
    assert_eq!(event["terminal"]["width"], 120);
    assert_eq!(event["terminal"]["height"], 40);
    assert_eq!(event["terminal"]["bytes"], "G1sySnJhdyBzb3VyY2UgYnl0ZXM=");
    bridge.detach("viewer", Some("term")).await;
    server
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tokio::test]
async fn terminal_bridge_two_viewer_isolation() -> Result<(), Box<dyn std::error::Error>> {
    use shprd_host::terminal_bridge::{
        AttachRequest, TerminalBridge, TerminalEvent, Transport, ViewerFrame,
    };
    use tokio::sync::mpsc;

    let home = tempfile::tempdir()?;
    let path = home.path().join("render.sock");
    let listener = UnixListener::bind(&path)?;
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await?;
        let mut wire = LengthDelimitedCodec::builder()
            .little_endian()
            .new_framed(socket);
        wire.next().await.ok_or("missing hello")??;
        wire.send(vec![0, 20, 1, 0].into()).await?;
        wire.next().await.ok_or("missing attach")??;
        while wire.next().await.transpose()?.is_some() {}
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    });
    let bridge = TerminalBridge::new(path);
    let (sender_a, mut events_a) = mpsc::unbounded_channel::<TerminalEvent>();
    let (sender_b, mut events_b) = mpsc::unbounded_channel::<TerminalEvent>();
    for (viewer_id, cols, rows, sender) in [
        ("attachment-a", 80, 24, sender_a),
        ("attachment-b", 120, 40, sender_b),
    ] {
        bridge
            .attach(AttachRequest {
                viewer_id: viewer_id.to_owned(),
                terminal_id: "term-1".to_owned(),
                cols,
                rows,
                transport: Transport::Direct(20),
                surface_cols: None,
                surface_rows: None,
                sender,
            })
            .await?;
    }
    bridge.input("attachment-a", "term-1", "YQ==", 100).await?;
    bridge.input("attachment-b", "term-1", "Yg==", 200).await?;
    bridge
        .publish_frame(ViewerFrame {
            terminal_id: "term-1".to_owned(),
            width: 120,
            height: 40,
            full: true,
            mouse_reporting: Some(true),
            bytes: b"frame".to_vec(),
        })
        .await;
    let frame_a = events_a.recv().await.ok_or("missing viewer A frame")?;
    assert_eq!(frame_a["terminal"]["width"], 120);
    assert_eq!(frame_a["terminal"]["height"], 40);
    assert_eq!(frame_a["terminal"]["bytes"], "ZnJhbWU=");
    assert_eq!(frame_a["connection_id"], "legacy-default");
    assert_eq!(frame_a["connection_generation"], 1);
    let frame_b = events_b.recv().await.ok_or("missing viewer B frame")?;
    assert_eq!(frame_b["terminal"]["width"], 120);
    assert_eq!(frame_b["terminal"]["height"], 40);
    assert_eq!(frame_b["terminal"]["bytes"], "ZnJhbWU=");
    assert_eq!(frame_b["connection_id"], "legacy-default");
    assert_eq!(frame_b["connection_generation"], 1);
    bridge.publish_clipboard("term-1", "owner-b", 300).await;
    assert!(events_a.try_recv().is_err());
    assert_eq!(
        events_b.recv().await.ok_or("missing owner clipboard")?["terminal_clipboard"]["data"],
        "owner-b"
    );
    bridge.detach("attachment-a", Some("term-1")).await;
    assert!(frame_a.publish().is_none());
    assert!(frame_b.publish().is_some());
    bridge
        .publish_frame(ViewerFrame {
            terminal_id: "term-1".to_owned(),
            width: 120,
            height: 40,
            full: true,
            mouse_reporting: None,
            bytes: b"still-live".to_vec(),
        })
        .await;
    let still_live = events_b
        .recv()
        .await
        .ok_or("viewer B retired unexpectedly")?;
    assert_eq!(still_live["terminal"]["bytes"], "c3RpbGwtbGl2ZQ==");
    assert_eq!(still_live["connection_id"], "legacy-default");
    assert_eq!(still_live["connection_generation"], 1);
    bridge.detach("attachment-b", Some("term-1")).await;
    let server_result = tokio::time::timeout(std::time::Duration::from_secs(1), server)
        .await
        .map_err(|error| error.to_string())?;
    let _: () = server_result
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[tokio::test]
async fn terminal_bridge_rejects_invalid_geometry_and_stale_actions()
-> Result<(), Box<dyn std::error::Error>> {
    use shprd_host::terminal_bridge::{
        AttachRequest, Error, ScrollRequest, TerminalBridge, Transport,
    };
    use tokio::sync::mpsc;

    let bridge = TerminalBridge::new(tempfile::tempdir()?.path().join("missing.sock"));
    let (sender, _) = mpsc::unbounded_channel();
    let attach = || AttachRequest {
        viewer_id: "attachment-a".to_owned(),
        terminal_id: "term-1".to_owned(),
        cols: 80,
        rows: 24,
        transport: Transport::Direct(20),
        surface_cols: Some(65_536),
        surface_rows: Some(24),
        sender: sender.clone(),
    };
    assert!(matches!(
        bridge.attach(attach()).await,
        Err(Error::InvalidSurface)
    ));
    assert!(matches!(
        bridge.resize("attachment-a", "term-1", 0, 24).await,
        Err(Error::InvalidSize)
    ));
    assert!(matches!(
        bridge.input("attachment-a", "term-1", "%%%", 0).await,
        Err(Error::InvalidInput)
    ));
    assert!(matches!(
        bridge
            .scroll(ScrollRequest {
                viewer_id: "attachment-a".to_owned(),
                terminal_id: "term-1".to_owned(),
                direction: "up".to_owned(),
                lines: 0,
                column: None,
                row: None,
                source: "wheel".to_owned(),
            })
            .await,
        Err(Error::InvalidScroll)
    ));
    bridge.detach("unknown-viewer", Some("term-1")).await;
    assert!(matches!(
        bridge.input("attachment-a", "term-1", "YQ==", 0).await,
        Err(Error::NotAttached)
    ));
    Ok(())
}

fn endpoint_surface_message(
    symbol: &str,
    revision: u64,
    mouse_reporting: bool,
) -> Result<Vec<u8>, bincode::error::EncodeError> {
    let cell = (symbol.to_owned(), 0_u32, 0_u32, 0_u16, false, None::<u32>);
    let frame = (
        vec![cell; 4],
        2_u16,
        2_u16,
        Some((1_u16, 1_u16, true, 1_u8)),
        Vec::<String>::new(),
        Vec::<u8>::new(),
    );
    let pane = (
        "pane-1",
        revision,
        (0_u16, 0_u16, 2_u16, 2_u16),
        (0_u16, 0_u16, 2_u16, 2_u16),
        None::<(u16, u16, u16, u16)>,
        Some((0_u64, 10_u64, 2_u16)),
        true,
        mouse_reporting,
        false,
        false,
        0_u16,
        0_u16,
    );
    let surface = bincode::encode_to_vec(
        ("boot-endpoint", revision, revision, frame, vec![pane]),
        bincode::config::standard(),
    )?;
    let mut message = bincode::encode_to_vec(13_u32, bincode::config::standard())?;
    message.extend_from_slice(&surface);
    Ok(message)
}

#[tokio::test]
async fn endpoint_bridge_consumes_unsolicited_events_and_interleaves_commands()
-> Result<(), Box<dyn std::error::Error>> {
    use base64::Engine;
    use shprd_host::{endpoint, terminal_bridge};
    use tokio::net::UnixListener;

    let directory = tempfile::tempdir()?;
    let endpoint_path = directory.path().join("endpoint.sock");
    let listener = UnixListener::bind(&endpoint_path)?;
    let fixture = tokio::spawn(async move {
        let (socket, _) = listener.accept().await?;
        let mut wire = LengthDelimitedCodec::builder()
            .little_endian()
            .new_framed(socket);
        let hello = wire.next().await.ok_or("missing endpoint hello")??;
        let ((tag, kind, data), _): ((u32, String, String), usize) =
            bincode::decode_from_slice(&hello, bincode::config::standard())?;
        assert_eq!((tag, kind.as_str()), (20, "endpoint.hello.v1"));
        let hello: serde_json::Value = serde_json::from_str(&data)?;
        assert_eq!(
            hello["surface_size"],
            serde_json::json!({"cols":2,"rows":2})
        );
        let welcome = serde_json::json!({
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
                serde_json::json!({"boot_id":"boot-endpoint","revision":1}),
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
        assert_eq!((tag, boot.as_str()), (15, "boot-endpoint"));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&data)?["method"],
            "pane.focus"
        );
        wire.send(endpoint_surface_message("A", 1, true)?.into())
            .await?;
        wire.send(
            bincode::encode_to_vec(
                (
                    18_u32,
                    "boot-endpoint",
                    "focus-1",
                    true,
                    b"{\"focused\":true}".to_vec(),
                ),
                bincode::config::standard(),
            )?
            .into(),
        )
        .await?;
        // Then an unsolicited surface arrives before any unrelated browser command.
        wire.send(endpoint_surface_message("B", 2, false)?.into())
            .await?;

        let mut saw_input = false;
        let mut saw_resize = false;
        while !saw_input || !saw_resize {
            let message = wire.next().await.ok_or("missing interleaved command")??;
            let (tag, _): (u32, usize) =
                bincode::decode_from_slice(&message, bincode::config::standard())?;
            match tag {
                13 => {
                    let ((tag, pane_id, count), _): ((u32, String, usize), usize) =
                        bincode::decode_from_slice(&message, bincode::config::standard())?;
                    assert_eq!((tag, pane_id.as_str(), count), (13, "pane-1", 1));
                    saw_input = true;
                }
                12 => {
                    let ((tag, x, y, cols, rows, pixels), _): (
                        (u32, u16, u16, u16, u16, bool),
                        usize,
                    ) = bincode::decode_from_slice(&message, bincode::config::standard())?;
                    assert_eq!((tag, x, y, cols, rows, pixels), (12, 0, 0, 2, 2, false));
                    saw_resize = true;
                }
                _ => return Err(format!("unexpected endpoint command tag {tag}").into()),
            }
        }
        wire.send(
            bincode::encode_to_vec((5_u32, "endpoint clipboard"), bincode::config::standard())?
                .into(),
        )
        .await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    });

    let endpoint = endpoint::Endpoint::connect(&endpoint_path, 2, 2).await?;
    let bridge = terminal_bridge::TerminalBridge::new(endpoint_path.clone());
    let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
    bridge
        .attach(terminal_bridge::AttachRequest {
            viewer_id: "viewer".to_owned(),
            terminal_id: "term-endpoint".to_owned(),
            cols: 2,
            rows: 2,
            transport: terminal_bridge::Transport::Endpoint {
                pane_id: "pane-1".to_owned(),
                endpoint: Box::new(endpoint),
            },
            surface_cols: None,
            surface_rows: None,
            sender,
        })
        .await?;

    let initial = tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            let event = events
                .recv()
                .await
                .ok_or("missing initial endpoint frame")?;
            let Some(payload) = event.publish() else {
                continue;
            };
            if payload.get("terminal").is_some() {
                break Ok::<_, Box<dyn std::error::Error>>(payload);
            }
        }
    })
    .await??;
    assert_eq!(initial["terminal"]["mouse_reporting"], true);
    let initial_bytes = base64::engine::general_purpose::STANDARD.decode(
        initial["terminal"]["bytes"]
            .as_str()
            .ok_or("initial bytes missing")?,
    )?;
    assert!(initial_bytes.contains(&b'A'));

    let unsolicited = tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            let event = events
                .recv()
                .await
                .ok_or("missing unsolicited endpoint frame")?;
            let Some(payload) = event.publish() else {
                continue;
            };
            if payload.get("terminal").is_some() {
                break Ok::<_, Box<dyn std::error::Error>>(payload);
            }
        }
    })
    .await??;
    assert_eq!(unsolicited["terminal"]["mouse_reporting"], false);
    let unsolicited_bytes = base64::engine::general_purpose::STANDARD.decode(
        unsolicited["terminal"]["bytes"]
            .as_str()
            .ok_or("unsolicited bytes missing")?,
    )?;
    assert!(unsolicited_bytes.contains(&b'B'));

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis()
        .try_into()?;
    tokio::try_join!(
        bridge.input("viewer", "term-endpoint", "YQ==", now_ms),
        bridge.resize("viewer", "term-endpoint", 2, 2),
    )?;

    let clipboard = tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            let event = events.recv().await.ok_or("missing endpoint clipboard")?;
            let Some(payload) = event.publish() else {
                continue;
            };
            if payload.get("terminal_clipboard").is_some() {
                break Ok::<_, Box<dyn std::error::Error>>(payload);
            }
        }
    })
    .await??;
    assert_eq!(
        clipboard["terminal_clipboard"]["data"],
        "endpoint clipboard"
    );

    let closed = tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            let event = events.recv().await.ok_or("missing endpoint close")?;
            let Some(payload) = event.publish() else {
                continue;
            };
            if payload.get("terminal_closed").is_some() {
                break Ok::<_, Box<dyn std::error::Error>>(payload);
            }
        }
    })
    .await??;
    assert_eq!(closed["terminal_closed"]["terminal_id"], "term-endpoint");
    assert!(events.try_recv().is_err());
    assert!(matches!(
        bridge
            .input("viewer", "term-endpoint", "YQ==", now_ms)
            .await,
        Err(terminal_bridge::Error::NotAttached)
    ));
    fixture
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;
    Ok(())
}
