//! HTTP surface for the native host.

use crate::{auth::Auth, connections, herdr, terminal_bridge::TerminalBridge};
use axum::{
    Json, Router,
    extract::{
        Request, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use shprd_connections::{ConnectionId, Manager, Profile, ProfileService, RpcRoute, resolve_rpc};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    sync::{Mutex, mpsc},
    task::{JoinHandle, JoinSet},
};
use tower_http::services::ServeDir;

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use shprd_connections::{Runtime, RuntimeContext, RuntimeFuture, SocketPaths};

    struct Ready;
    impl Runtime for Ready {
        fn start<'a>(&'a self, _: &'a RuntimeContext) -> RuntimeFuture<'a, SocketPaths> {
            Box::pin(async {
                Ok(SocketPaths {
                    control: "/fixture/control".into(),
                    render: "/fixture/render".into(),
                })
            })
        }
        fn stop(&self) -> RuntimeFuture<'_, ()> {
            Box::pin(async { Ok(()) })
        }
    }

    #[tokio::test]
    async fn queued_payloads_lose_retired_data_at_publication()
    -> Result<(), Box<dyn std::error::Error>> {
        // Given queued reply and event from a ready runtime.
        let profile = Profile::legacy("/fixture/control", "/fixture/render")?;
        let id = profile.id().clone();
        let manager = Manager::new(id.clone());
        manager.register(profile, Arc::new(|_| Ok(Arc::new(Ready))))?;
        manager.connect(&id).await?;
        let lease = manager.lease(&id)?;
        let (send, mut received) = mpsc::channel(2);
        for payload in [
            json!({"id":"pending","result":{"private":"retired"}}),
            json!({"event":"agent_control.event","data":{"private":"retired"}}),
        ] {
            send.send(Outgoing {
                payload,
                lease: Some(lease.clone()),
            })
            .await
            .map_err(|_| "closed queue")?;
        }
        // When retirement happens after enqueue but before the socket publisher reads.
        manager.disconnect(&id).await?;
        // Then no retired data leaves; the outstanding request receives one error.
        let reply = received.try_recv()?.publish().ok_or("missing reply")?;
        assert_eq!(reply["id"], "pending");
        assert!(reply.get("error").is_some());
        assert!(reply.get("result").is_none());
        assert!(received.try_recv()?.publish().is_none());
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn terminal_host_removes_empty_bridges_after_repeated_attach_detach()
    -> Result<(), Box<dyn std::error::Error>> {
        use tokio::net::UnixListener;
        use tokio_util::codec::LengthDelimitedCodec;

        let directory = tempfile::tempdir()?;
        let render_path = directory.path().join("render.sock");
        let listener = UnixListener::bind(&render_path)?;
        let fixture = tokio::spawn(async move {
            for _ in 0..3_u8 {
                let (socket, _) = listener.accept().await?;
                let mut wire = LengthDelimitedCodec::builder()
                    .little_endian()
                    .new_framed(socket);
                let hello = wire.next().await.ok_or("missing terminal hello")??;
                assert_eq!(hello.as_ref(), &[0, 20, 80, 24, 0, 0, 1, 0, 2]);
                wire.send(vec![0, 20, 1, 0].into()).await?;
                let attach = wire.next().await.ok_or("missing terminal attach")??;
                let ((tag, terminal_id, takeover), _): ((u32, String, bool), usize) =
                    bincode::decode_from_slice(&attach, bincode::config::standard())?;
                assert_eq!(
                    (tag, terminal_id.as_str(), takeover),
                    (5, "term-lifecycle", true)
                );
                while wire.next().await.transpose()?.is_some() {}
            }
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        });
        let host = Host {
            socket: render_path,
            auth: Auth::new(false, String::new())?,
            attachments: None,
            profiles: None,
            manager: None,
            terminals: Mutex::new(HashMap::new()),
            terminal_lifecycle: Mutex::new(()),
        };
        let (terminal_events, _) = mpsc::unbounded_channel();
        for cycle in 0..3_u8 {
            let request = json!({
                "id": format!("attach-{cycle}"),
                "method": "terminal.attach",
                "params": {"terminal_id":"term-lifecycle","cols":80,"rows":24}
            });
            let mut lease = None;
            let response = terminal_rpc(
                &host,
                &request,
                "terminal.attach",
                request["id"].as_str(),
                "viewer-lifecycle",
                &terminal_events,
                &mut lease,
            )
            .await;
            assert_eq!(response["result"], json!({"ok":true}));
            assert_eq!(host.terminals.lock().await.len(), 1);

            let request = json!({
                "id": format!("detach-{cycle}"),
                "method": "terminal.detach",
                "params": {"terminal_id":"term-lifecycle"}
            });
            let response = terminal_rpc(
                &host,
                &request,
                "terminal.detach",
                request["id"].as_str(),
                "viewer-lifecycle",
                &terminal_events,
                &mut lease,
            )
            .await;
            assert_eq!(response["result"], json!({"ok":true}));
            assert_eq!(host.terminals.lock().await.len(), 0);
        }
        tokio::time::timeout(std::time::Duration::from_secs(1), fixture)
            .await??
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    #[tokio::test]
    async fn queued_terminal_events_lose_retired_generation_at_publication()
    -> Result<(), Box<dyn std::error::Error>> {
        use shprd_connections::{ConnectionId, Manager, Profile};
        use std::sync::Arc;

        let profile = Profile::from_value(json!({
            "id": "scoped-terminal",
            "label": "Scoped terminal",
            "type": "local",
            "control_socket_path": "/fixture/control",
            "client_socket_path": "/fixture/render",
            "auto_connect": true,
        }))?;
        let id: ConnectionId = profile.id().clone();
        let manager = Arc::new(Manager::new(id.clone()));
        manager.register(profile, Arc::new(|_| Ok(Arc::new(Ready))))?;
        manager.connect(&id).await?;
        let old = manager.lease(&id)?;
        let (send, mut received) = mpsc::unbounded_channel();
        for payload in [
            json!({"terminal":{"terminal_id":"old-frame","bytes":"b2xk"}}),
            json!({"terminal_clipboard":{"terminal_id":"term","data":"old-clipboard"}}),
            json!({"terminal_closed":{"terminal_id":"term","reason":"old-closed"}}),
        ] {
            send.send(crate::terminal_bridge::TerminalEvent::new(
                payload,
                Some(&old),
            ))
            .map_err(|_| "closed terminal queue")?;
        }
        manager.disconnect(&id).await?;
        manager.connect(&id).await?;
        let current = manager.lease(&id)?;
        send.send(crate::terminal_bridge::TerminalEvent::new(
            json!({"terminal":{"terminal_id":"new-frame","bytes":"bmV3"}}),
            Some(&current),
        ))
        .map_err(|_| "closed terminal queue")?;

        assert_eq!(old.generation() + 2, current.generation());
        assert_eq!(received.try_recv()?.publish(), None);
        assert_eq!(received.try_recv()?.publish(), None);
        assert_eq!(received.try_recv()?.publish(), None);
        let next = received.try_recv()?.publish().ok_or("new event retired")?;
        assert_eq!(next["terminal"]["terminal_id"], "new-frame");
        assert_eq!(next["connection_id"], json!(id));
        assert_eq!(next["connection_generation"], current.generation());
        Ok(())
    }

    struct TerminalRuntime(PathBuf);

    impl Runtime for TerminalRuntime {
        fn start<'a>(&'a self, _: &'a RuntimeContext) -> RuntimeFuture<'a, SocketPaths> {
            Box::pin(async {
                Ok(SocketPaths {
                    control: self.0.with_extension("control"),
                    render: self.0.clone(),
                })
            })
        }

        fn stop(&self) -> RuntimeFuture<'_, ()> {
            Box::pin(async { Ok(()) })
        }
    }

    async fn terminal_host_with_manager(
        render: PathBuf,
    ) -> Result<(Arc<Host>, Arc<Manager>), Box<dyn std::error::Error + Send + Sync>> {
        let manager = Arc::new(Manager::new(ConnectionId::parse("alpha")?));
        for id in ["alpha", "beta"] {
            let profile = Profile::from_value(json!({
                "id":id,"label":id,"type":"local",
                "control_socket_path":"/fixture/control","client_socket_path":render,
                "auto_connect":true
            }))?;
            let runtime = Arc::new(TerminalRuntime(render.clone()));
            manager.register(profile, Arc::new(move |_| Ok(runtime.clone())))?;
            manager.connect(&ConnectionId::parse(id)?).await?;
        }
        Ok((
            Arc::new(Host {
                socket: render,
                auth: Auth::new(false, String::new())?,
                attachments: None,
                profiles: None,
                manager: Some(Arc::clone(&manager)),
                terminals: Mutex::new(HashMap::new()),
                terminal_lifecycle: Mutex::new(()),
            }),
            manager,
        ))
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn terminal_generation_replacement_retires_old_bridge_with_viewer_connected()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use tokio::net::{TcpListener, UnixListener};
        use tokio_util::codec::LengthDelimitedCodec;

        // Given two live connection generations sharing a render path and browser.
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("render.sock");
        let render = UnixListener::bind(&path)?;
        let control = UnixListener::bind(path.with_extension("control"))?;
        let control_fixture = async {
            use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
            for _ in 0..3 {
                let (socket, _) = control.accept().await?;
                let mut socket = BufReader::new(socket);
                let mut line = String::new();
                socket.read_line(&mut line).await?;
                let request: Value = serde_json::from_str(&line)?;
                assert_eq!(request["method"], "ping");
                let response = json!({"id":request["id"],"result":{"protocol":20}});
                socket
                    .get_mut()
                    .write_all(format!("{response}\n").as_bytes())
                    .await?;
            }
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        };
        let (host, manager) = terminal_host_with_manager(path).await?;
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let router = Router::new()
            .route("/ws", get(websocket))
            .with_state(Arc::clone(&host));
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let (closed, mut closures) = mpsc::unbounded_channel();
        let server = async {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        };
        let fixture = async {
            let mut peers = JoinSet::new();
            for index in 0..3 {
                let (socket, _) = render.accept().await?;
                let closed = closed.clone();
                peers.spawn(async move {
                    let mut wire = LengthDelimitedCodec::builder()
                        .little_endian()
                        .new_framed(socket);
                    assert_eq!(
                        wire.next().await.ok_or("missing hello")??.as_ref(),
                        &[0, 20, 80, 24, 0, 0, 1, 0, 2]
                    );
                    wire.send(vec![0, 20, 1, 0].into()).await?;
                    let attach = wire.next().await.ok_or("missing attach")??;
                    let ((tag, id, takeover), _): ((u32, String, bool), usize) =
                        bincode::decode_from_slice(&attach, bincode::config::standard())?;
                    assert_eq!((tag, id.as_str(), takeover), (5, "term", true));
                    while let Some(bytes) = wire.next().await.transpose()? {
                        let ((tag, data), _): ((u32, Vec<u8>), usize) =
                            bincode::decode_from_slice(&bytes, bincode::config::standard())?;
                        assert_eq!((tag, data.as_slice()), (1, b"live".as_slice()));
                        wire.send(
                            bincode::encode_to_vec(
                                (2_u32, 1_u64, 80_u16, 24_u16, true, data),
                                bincode::config::standard(),
                            )?
                            .into(),
                        )
                        .await?;
                    }
                    closed.send(index)?;
                    Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
                });
            }
            while let Some(peer) = peers.join_next().await {
                peer??;
            }
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        };
        let client = async {
            let (mut browser, _) =
                tokio_tungstenite::connect_async(format!("ws://{address}/ws")).await?;
            browser.next().await.ok_or("missing browser hello")??;
            for (connection, replacement) in [("alpha", false), ("beta", false), ("alpha", true)] {
                if replacement {
                    // When alpha is replaced without detaching either browser viewer.
                    let id = ConnectionId::parse("alpha")?;
                    manager.disconnect(&id).await?;
                    manager.connect(&id).await?;
                }
                let generation = manager
                    .lease(&ConnectionId::parse(connection)?)?
                    .generation();
                browser
                    .send(tokio_tungstenite::tungstenite::Message::Text(
                        json!({
                            "id":"attach","method":"terminal.attach","connection_id":connection,
                            "connection_generation":generation,
                            "params":{"terminal_id":"term","cols":80,"rows":24}
                        })
                        .to_string()
                        .into(),
                    ))
                    .await?;
                let reply: Value = serde_json::from_str(
                    browser
                        .next()
                        .await
                        .ok_or("missing attach reply")??
                        .to_text()?,
                )?;
                assert_eq!(reply["result"], json!({"ok":true}));
                assert_eq!(reply["connection_generation"], generation);
            }
            // Then only current bridges remain; the retired socket closes autonomously.
            assert_eq!(host.terminals.lock().await.len(), 2);
            assert_eq!(closures.recv().await, Some(0));
            for (connection, generation) in [("alpha", 3), ("beta", 1)] {
                browser
                    .send(tokio_tungstenite::tungstenite::Message::Text(
                        json!({
                            "id":"input","method":"terminal.input","connection_id":connection,
                            "connection_generation":generation,
                            "params":{"terminal_id":"term","data":"bGl2ZQ=="}
                        })
                        .to_string()
                        .into(),
                    ))
                    .await?;
                let mut replied = false;
                let mut framed = false;
                while !replied || !framed {
                    let event: Value = serde_json::from_str(
                        browser
                            .next()
                            .await
                            .ok_or("missing live output")??
                            .to_text()?,
                    )?;
                    assert_eq!(event["connection_id"], connection);
                    assert_eq!(event["connection_generation"], generation);
                    if event["id"] == "input" {
                        assert_eq!(event["result"], json!({"ok":true}));
                        replied = true;
                    } else {
                        assert_eq!(event["terminal"]["bytes"], "bGl2ZQ==");
                        framed = true;
                    }
                }
            }
            browser.close(None).await?;
            stop.send(()).map_err(|()| "shutdown observer lost")?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        };
        tokio::time::timeout(Duration::from_secs(3), async {
            tokio::try_join!(client, fixture, server, control_fixture)
        })
        .await??;
        Ok(())
    }

    #[tokio::test]
    async fn terminal_generation_retirement_precedes_stale_detach_rejection()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Given an old registry entry whose connection has been retired.
        let directory = tempfile::tempdir()?;
        let (host, manager) =
            terminal_host_with_manager(directory.path().join("render.sock")).await?;
        let id = ConnectionId::parse("alpha")?;
        let lease = manager.lease(&id)?;
        host.terminals.lock().await.insert(
            "retired".into(),
            Arc::new(TerminalBridge::with_lease(
                lease.paths.render.clone(),
                Some(lease.clone()),
            )),
        );
        manager.disconnect(&id).await?;
        let request = json!({
            "id":"detach","method":"terminal.detach","connection_id":id,
            "connection_generation":lease.generation(),"params":{"terminal_id":"term"}
        });
        let (events, _) = mpsc::unbounded_channel();
        let mut outgoing = None;
        // When the old viewer sends detach after connection retirement.
        let reply = tokio::time::timeout(
            Duration::from_secs(1),
            terminal_rpc(
                &host,
                &request,
                "terminal.detach",
                Some("detach"),
                "viewer",
                &events,
                &mut outgoing,
            ),
        )
        .await?;
        // Then routing still rejects the request, but old host ownership is gone.
        assert!(reply.get("error").is_some());
        assert!(reply.get("result").is_none());
        assert!(host.terminals.lock().await.is_empty());
        Ok(())
    }

    struct ControlledSink {
        state: Arc<ControlledSinkState>,
    }

    #[derive(Clone)]
    struct ControlledSinkHandle {
        state: Arc<ControlledSinkState>,
    }

    struct ControlledSinkState {
        released: std::sync::atomic::AtomicBool,
        pending_reported: std::sync::atomic::AtomicBool,
        pending_sender: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
        waker: futures_util::task::AtomicWaker,
        sent: std::sync::Mutex<Vec<Message>>,
        flushes: std::sync::atomic::AtomicUsize,
    }

    impl ControlledSink {
        fn new() -> (
            Self,
            ControlledSinkHandle,
            tokio::sync::oneshot::Receiver<()>,
        ) {
            let (pending_sender, pending_receiver) = tokio::sync::oneshot::channel();
            let state = Arc::new(ControlledSinkState {
                released: std::sync::atomic::AtomicBool::new(false),
                pending_reported: std::sync::atomic::AtomicBool::new(false),
                pending_sender: std::sync::Mutex::new(Some(pending_sender)),
                waker: futures_util::task::AtomicWaker::new(),
                sent: std::sync::Mutex::new(Vec::new()),
                flushes: std::sync::atomic::AtomicUsize::new(0),
            });
            (
                Self {
                    state: Arc::clone(&state),
                },
                ControlledSinkHandle { state },
                pending_receiver,
            )
        }
    }

    impl ControlledSinkHandle {
        fn release(&self) {
            self.state
                .released
                .store(true, std::sync::atomic::Ordering::Release);
            self.state.waker.wake();
        }

        fn take_sent(&self) -> Vec<Message> {
            std::mem::take(
                &mut *self
                    .state
                    .sent
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()),
            )
        }

        fn flushes(&self) -> usize {
            self.state
                .flushes
                .load(std::sync::atomic::Ordering::Acquire)
        }
    }

    impl futures_util::Sink<Message> for ControlledSink {
        type Error = std::io::Error;

        fn poll_ready(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            let state = &self.state;
            if state.released.load(std::sync::atomic::Ordering::Acquire) {
                return std::task::Poll::Ready(Ok(()));
            }
            state.waker.register(cx.waker());
            if state.released.load(std::sync::atomic::Ordering::Acquire) {
                return std::task::Poll::Ready(Ok(()));
            }
            if !state
                .pending_reported
                .swap(true, std::sync::atomic::Ordering::AcqRel)
            {
                if let Some(sender) = state
                    .pending_sender
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .take()
                {
                    let _ = sender.send(());
                }
            }
            std::task::Poll::Pending
        }

        fn start_send(self: std::pin::Pin<&mut Self>, item: Message) -> Result<(), Self::Error> {
            assert!(
                self.state
                    .released
                    .load(std::sync::atomic::Ordering::Acquire),
                "start_send before controlled readiness release"
            );
            self.state
                .sent
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(item);
            Ok(())
        }

        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            self.state
                .flushes
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            std::task::Poll::Ready(Ok(()))
        }

        fn poll_close(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn send_paths_drop_retired_payloads_after_pending_readiness()
    -> Result<(), Box<dyn std::error::Error>> {
        use shprd_connections::{Manager, Profile};

        // Given real Manager leases and production send paths blocked at poll_ready.
        let profile = Profile::from_value(json!({
            "id": "scoped-backpressure",
            "label": "Scoped backpressure",
            "type": "local",
            "control_socket_path": "/fixture/control",
            "client_socket_path": "/fixture/render",
            "auto_connect": true,
        }))?;
        let id = profile.id().clone();
        let manager = Arc::new(Manager::new(id.clone()));
        manager.register(profile, Arc::new(|_| Ok(Arc::new(Ready))))?;
        manager.connect(&id).await?;
        let old = manager.lease(&id)?;

        let (frame_sink, frame_control, frame_pending) = ControlledSink::new();
        let (clipboard_sink, clipboard_control, clipboard_pending) = ControlledSink::new();
        let (closed_sink, closed_control, closed_pending) = ControlledSink::new();
        let (reply_sink, reply_control, reply_pending) = ControlledSink::new();
        let frame_lease = old.clone();
        let frame_task = tokio::spawn(async move {
            let mut sink = frame_sink;
            send_terminal(
                &mut sink,
                crate::terminal_bridge::TerminalEvent::new(
                    json!({"terminal":{"terminal_id":"term","bytes":"cmV0aXJlZC1mcmFtZQ=="}}),
                    Some(&frame_lease),
                ),
            )
            .await
        });
        let clipboard_lease = old.clone();
        let clipboard_task = tokio::spawn(async move {
            let mut sink = clipboard_sink;
            send_terminal(
                &mut sink,
                crate::terminal_bridge::TerminalEvent::new(
                    json!({"terminal_clipboard":{"terminal_id":"term","data":"retired clipboard"}}),
                    Some(&clipboard_lease),
                ),
            )
            .await
        });
        let closed_lease = old.clone();
        let closed_task = tokio::spawn(async move {
            let mut sink = closed_sink;
            send_terminal(
                &mut sink,
                crate::terminal_bridge::TerminalEvent::new(
                    json!({"terminal_closed":{"terminal_id":"term","reason":"retired closed"}}),
                    Some(&closed_lease),
                ),
            )
            .await
        });
        let reply_lease = old.clone();
        let reply_task = tokio::spawn(async move {
            let mut sink = reply_sink;
            send_outgoing(
                &mut sink,
                Outgoing {
                    payload: json!({"id":"request","result":{"private":"retired result"}}),
                    lease: Some(reply_lease),
                },
            )
            .await
        });

        // When every production send path reports Pending, retire lease before releasing it.
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            tokio::try_join!(
                frame_pending,
                clipboard_pending,
                closed_pending,
                reply_pending
            )
        })
        .await??;
        manager.disconnect(&id).await?;
        frame_control.release();
        clipboard_control.release();
        closed_control.release();
        reply_control.release();
        tokio::time::timeout(std::time::Duration::from_secs(1), frame_task).await???;
        tokio::time::timeout(std::time::Duration::from_secs(1), clipboard_task).await???;
        tokio::time::timeout(std::time::Duration::from_secs(1), closed_task).await???;
        tokio::time::timeout(std::time::Duration::from_secs(1), reply_task).await???;

        // Then no retired terminal bytes reach start_send, while stale reply becomes one error.
        assert!(frame_control.take_sent().is_empty());
        assert!(clipboard_control.take_sent().is_empty());
        assert!(closed_control.take_sent().is_empty());
        assert_eq!(frame_control.flushes(), 0);
        assert_eq!(clipboard_control.flushes(), 0);
        assert_eq!(closed_control.flushes(), 0);
        let reply_messages = reply_control.take_sent();
        assert_eq!(reply_messages.len(), 1);
        let reply: Value = serde_json::from_str(reply_messages[0].to_text()?)?;
        assert_eq!(reply["id"], "request");
        assert_eq!(reply["connection_id"], json!(id));
        assert_eq!(reply["connection_generation"], old.generation());
        assert_eq!(
            reply["error"]["message"],
            "connection changed during request"
        );
        assert!(reply.get("result").is_none());
        assert_eq!(reply_control.flushes(), 1);

        // Current generation remains publishable after deterministic readiness release.
        manager.connect(&id).await?;
        let current = manager.lease(&id)?;
        assert!(current.generation() > old.generation());
        let (current_frame_sink, current_frame_control, current_frame_pending) =
            ControlledSink::new();
        let (current_reply_sink, current_reply_control, current_reply_pending) =
            ControlledSink::new();
        let current_frame_lease = current.clone();
        let current_frame_task = tokio::spawn(async move {
            let mut sink = current_frame_sink;
            send_terminal(
                &mut sink,
                crate::terminal_bridge::TerminalEvent::new(
                    json!({"terminal":{"terminal_id":"term","bytes":"Y3VycmVudA=="}}),
                    Some(&current_frame_lease),
                ),
            )
            .await
        });
        let current_reply_lease = current.clone();
        let current_reply_id = id.clone();
        let current_reply_generation = current.generation();
        let current_reply_task = tokio::spawn(async move {
            let mut sink = current_reply_sink;
            send_outgoing(
                &mut sink,
                Outgoing {
                    payload: json!({
                        "id":"current",
                        "connection_id":current_reply_id,
                        "connection_generation":current_reply_generation,
                        "result":{"private":"current result"}
                    }),
                    lease: Some(current_reply_lease),
                },
            )
            .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            tokio::try_join!(current_frame_pending, current_reply_pending)
        })
        .await??;
        current_frame_control.release();
        current_reply_control.release();
        tokio::time::timeout(std::time::Duration::from_secs(1), current_frame_task).await???;
        tokio::time::timeout(std::time::Duration::from_secs(1), current_reply_task).await???;

        let current_frame = current_frame_control.take_sent();
        assert_eq!(current_frame.len(), 1);
        let current_frame: Value = serde_json::from_str(current_frame[0].to_text()?)?;
        assert_eq!(current_frame["terminal"]["bytes"], "Y3VycmVudA==");
        assert_eq!(current_frame["connection_id"], json!(id));
        assert_eq!(current_frame["connection_generation"], current.generation());
        assert_eq!(current_frame_control.flushes(), 1);
        let current_reply = current_reply_control.take_sent();
        assert_eq!(current_reply.len(), 1);
        let current_reply: Value = serde_json::from_str(current_reply[0].to_text()?)?;
        assert_eq!(current_reply["id"], "current");
        assert_eq!(current_reply["result"]["private"], "current result");
        assert_eq!(current_reply["connection_id"], json!(id));
        assert_eq!(current_reply["connection_generation"], current.generation());
        assert_eq!(current_reply_control.flushes(), 1);
        Ok(())
    }
}

static VIEWER_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Outgoing {
    payload: serde_json::Value,
    lease: Option<shprd_connections::Lease>,
}

impl Outgoing {
    fn publish(self) -> Option<serde_json::Value> {
        if let Some(lease) = &self.lease {
            if !lease.is_current() {
                return self.payload.get("id").map(|id| json!({
                    "id":id,"connection_id":lease.connection_id,"connection_generation":lease.generation(),
                    "error":{"message":"connection changed during request"}
                }));
            }
        }
        Some(self.payload)
    }
}

async fn send_outgoing<S>(socket: &mut S, outgoing: Outgoing) -> Result<(), S::Error>
where
    S: futures_util::Sink<Message> + Unpin,
{
    use futures_util::SinkExt;
    use std::pin::Pin;
    // Check retirement after backpressure clears, immediately before handing bytes to the sink.
    std::future::poll_fn(|cx| Pin::new(&mut *socket).poll_ready(cx)).await?;
    if let Some(payload) = outgoing.publish() {
        Pin::new(&mut *socket).start_send(Message::Text(payload.to_string().into()))?;
        socket.flush().await?;
    }
    Ok(())
}

async fn send_terminal<S>(
    socket: &mut S,
    event: crate::terminal_bridge::TerminalEvent,
) -> Result<(), S::Error>
where
    S: futures_util::Sink<Message> + Unpin,
{
    use futures_util::SinkExt;
    use std::pin::Pin;
    // Check lease after backpressure clears, immediately before handing bytes to the sink.
    std::future::poll_fn(|cx| Pin::new(&mut *socket).poll_ready(cx)).await?;
    if let Some(payload) = event.publish() {
        Pin::new(&mut *socket).start_send(Message::Text(payload.to_string().into()))?;
        socket.flush().await?;
    }
    Ok(())
}

struct Host {
    socket: PathBuf,
    auth: Auth,
    attachments: Option<PathBuf>,
    profiles: Option<tokio::sync::RwLock<ProfileService>>,
    manager: Option<Arc<Manager>>,
    terminals: Mutex<HashMap<String, Arc<TerminalBridge>>>,
    terminal_lifecycle: Mutex<()>,
}

pub fn router() -> axum::Router {
    axum::Router::new()
        .route("/health", axum::routing::get(|| async { "Ok" }))
        .route("/healthz", axum::routing::get(|| async { "Ok" }))
}

pub fn configured_router(socket: PathBuf, public_dir: PathBuf, auth: Auth) -> Router {
    configured_router_with_attachments(
        socket,
        public_dir,
        auth,
        shprd_agent::default_directory().ok(),
    )
}

pub fn configured_router_with_attachments(
    socket: PathBuf,
    public_dir: PathBuf,
    auth: Auth,
    attachments: Option<PathBuf>,
) -> Router {
    build_router(socket, public_dir, auth, attachments, None, None)
}

pub fn configured_router_with_profiles(
    socket: PathBuf,
    public_dir: PathBuf,
    auth: Auth,
    attachments: Option<PathBuf>,
    profiles: ProfileService,
    manager: Arc<Manager>,
) -> Router {
    build_router(
        socket,
        public_dir,
        auth,
        attachments,
        Some(profiles),
        Some(manager),
    )
}

fn build_router(
    socket: PathBuf,
    public_dir: PathBuf,
    auth: Auth,
    attachments: Option<PathBuf>,
    profiles: Option<ProfileService>,
    manager: Option<Arc<Manager>>,
) -> Router {
    let state = Arc::new(Host {
        socket,
        auth,
        attachments,
        profiles: profiles.map(tokio::sync::RwLock::new),
        manager,
        terminals: Mutex::new(HashMap::new()),
        terminal_lifecycle: Mutex::new(()),
    });
    let protected = Router::new()
        .route("/ws", get(websocket))
        .route("/api/health", get(health))
        .route("/api/herdr-info", get(herdr_info))
        .route(
            "/api/connections/{connection_id}/herdr-info",
            get(herdr_info),
        )
        .fallback_service(ServeDir::new(public_dir))
        .layer(middleware::from_fn_with_state(
            Arc::clone(&state),
            authenticate,
        ));
    protected
        .route("/api/login", post(login))
        .route("/login", get(|| async { axum::response::Html(LOGIN) }))
        .with_state(state)
        .merge(router())
}

async fn authenticate(State(state): State<Arc<Host>>, request: Request, next: Next) -> Response {
    let cookie = request
        .headers()
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok());
    if !state.auth.authenticated(cookie) {
        if request.method() == Method::GET
            && request
                .headers()
                .get(header::ACCEPT)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.contains("text/html"))
        {
            return (StatusCode::FOUND, [(header::LOCATION, "/login")]).into_response();
        }
        return StatusCode::UNAUTHORIZED.into_response();
    }
    next.run(request).await
}

async fn health(State(state): State<Arc<Host>>) -> Json<serde_json::Value> {
    Json(json!({"ok":true,"version":env!("CARGO_PKG_VERSION"),"socket":state.socket}))
}

async fn herdr_info(State(state): State<Arc<Host>>, request: Request) -> Response {
    let lease = if let Some(manager) = &state.manager {
        let route =
            shprd_connections::parse_http_route(request.uri().path(), request.method().as_str());
        let query: Result<HashMap<String, String>, _> =
            axum::extract::Query::try_from_uri(request.uri()).map(|value| value.0);
        let resolved = route.and_then(|route| {
            let route = route.ok_or_else(|| {
                shprd_connections::Error::Invalid("invalid connection route".into())
            })?;
            let query =
                query.map_err(|_| shprd_connections::Error::Invalid("invalid query".into()))?;
            let generation = shprd_connections::query_generation(
                query.get("connection_generation").map(String::as_str),
            )?;
            manager.resolve(route.connection_id.as_ref(), generation)
        });
        match resolved {
            Ok(lease) => Some(lease),
            Err(error) => {
                let status = match &error {
                    shprd_connections::Error::Routing { status, .. } => {
                        StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_REQUEST)
                    }
                    shprd_connections::Error::Stale => StatusCode::CONFLICT,
                    _ => StatusCode::BAD_REQUEST,
                };
                return (
                    status,
                    Json(json!({"error":shprd_connections::sanitize_error(&error.to_string())})),
                )
                    .into_response();
            }
        }
    } else {
        None
    };
    let path = lease
        .as_ref()
        .map(|lease| &lease.paths.control)
        .unwrap_or(&state.socket);
    let result = herdr::call(path, "ping", &json!({}), Duration::from_secs(8)).await;
    if lease.as_ref().is_some_and(|lease| !lease.is_current()) {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error":"connection changed during request"})),
        )
            .into_response();
    }
    let mut response = match result {
        Ok(info) => Json(json!({"version":info.get("version"),"protocol":info.get("protocol")}))
            .into_response(),
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":shprd_connections::sanitize_error(&error.to_string())})),
        )
            .into_response(),
    };
    if let Some(lease) = lease {
        for (name, value) in shprd_connections::response_headers(&lease) {
            if let (Ok(name), Ok(value)) = (
                axum::http::HeaderName::try_from(name),
                axum::http::HeaderValue::try_from(value),
            ) {
                response.headers_mut().insert(name, value);
            }
        }
    }
    response
}

#[derive(Deserialize)]
struct Login {
    password: String,
}

async fn login(State(state): State<Arc<Host>>, request: Request) -> Response {
    if !state.auth.required() {
        return Json(json!({"ok":true,"note":"auth not required"})).into_response();
    }
    let bytes = match axum::body::to_bytes(request.into_body(), 16 * 1024).await {
        Ok(bytes) => bytes,
        Err(_) => return StatusCode::PAYLOAD_TOO_LARGE.into_response(),
    };
    let input: Login = match serde_json::from_slice(&bytes) {
        Ok(input) => input,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error":"bad request"})),
            )
                .into_response();
        }
    };
    match state.auth.login(&input.password) {
        Ok(Some(cookie)) => {
            ([(header::SET_COOKIE, cookie)], Json(json!({"ok":true}))).into_response()
        }
        Ok(None) => (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"wrong password"})),
        )
            .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn websocket(State(state): State<Arc<Host>>, upgrade: WebSocketUpgrade) -> Response {
    upgrade
        .max_message_size(1024 * 1024)
        .on_upgrade(|socket| websocket_session(state, socket))
}

async fn websocket_session(state: Arc<Host>, mut socket: WebSocket) {
    let default_id = match &state.manager {
        Some(manager) => match manager.default_id() {
            Ok(id) => id.as_str().to_owned(),
            Err(_) => return,
        },
        None => "legacy-default".to_owned(),
    };
    let hello = json!({"hello":true,"socket":state.socket,"bridge_protocol_version":2,"default_connection_id":default_id, "capabilities":{"connection_id":true,"connection_scoped_http":false,"connection_runtime_generation":true,"native_agents":state.attachments.is_some()}});
    if socket
        .send(Message::Text(hello.to_string().into()))
        .await
        .is_err()
    {
        return;
    }
    let mut requests = JoinSet::new();
    let mut mutations = JoinSet::new();
    let subscriptions = Arc::new(Mutex::new(HashMap::<String, JoinHandle<()>>::new()));
    let viewer_id = format!(
        "viewer-{}-{}",
        now_ms(),
        VIEWER_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    );
    let (terminal_events, mut terminal_received) =
        mpsc::unbounded_channel::<crate::terminal_bridge::TerminalEvent>();
    let (events, mut received) = mpsc::channel::<Outgoing>(128);
    loop {
        tokio::select! {
            message = socket.recv() => {
                match message {
                    Some(Ok(Message::Text(text))) => {
                        if requests.len() + mutations.len() >= 128 {
                            let _ = socket.send(Message::Close(None)).await;
                            break;
                        }
                        let state = Arc::clone(&state);
                        let subscriptions = Arc::clone(&subscriptions);
                        let events = events.clone();
                        let terminal_events = terminal_events.clone();
                        let viewer_id = viewer_id.clone();
                        let request = serde_json::from_str::<serde_json::Value>(&text);
                        let durable = request.as_ref().ok().and_then(|request| request.get("method")).and_then(serde_json::Value::as_str).is_some_and(|method| matches!(method,
                            "connections.create" | "connections.update" | "connections.remove" | "connections.set_default" | "connections.connect" | "connections.disconnect"));
                        let tasks = if durable { &mut mutations } else { &mut requests };
                        tasks.spawn(async move {
                            let mut lease = None;
                            let payload = match request {
                                Ok(request) => rpc(&state, &request, &subscriptions, &events, &mut lease, &viewer_id, &terminal_events).await,
                                Err(_) => json!({"id":null,"error":{"message":"bad json"}}),
                            };
                            Outgoing {payload, lease}
                        });
                    }
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    Some(Ok(Message::Binary(_) | Message::Ping(_) | Message::Pong(_))) => {}
                }
            }
            result = requests.join_next(), if !requests.is_empty() => {
                match result {
                    Some(Ok(result)) => {
                        if send_outgoing(&mut socket, result).await.is_err() {break;}
                    }
                    Some(Err(error)) => {eprintln!("WebSocket request task: {error}");break;}
                    None => {}
                }
            }
            Some(event) = received.recv() => {
                if send_outgoing(&mut socket, event).await.is_err() { break; }
            }
            Some(event) = terminal_received.recv() => {
                if send_terminal(&mut socket, event).await.is_err() { break; }
            }
            result = mutations.join_next(), if !mutations.is_empty() => {
                match result {
                    Some(Ok(result)) => {
                        if send_outgoing(&mut socket, result).await.is_err() {break;}
                    }
                    Some(Err(error)) => {eprintln!("WebSocket mutation task: {error}");break;}
                    None => {}
                }
            }
        }
    }
    drop(socket);
    requests.shutdown().await;
    let terminal_lifecycle = state.terminal_lifecycle.lock().await;
    let bridges: Vec<(String, Arc<TerminalBridge>)> = state
        .terminals
        .lock()
        .await
        .iter()
        .map(|(key, bridge)| (key.clone(), Arc::clone(bridge)))
        .collect();
    for (key, bridge) in bridges {
        if bridge.detach_and_is_empty(&viewer_id, None).await {
            let removed = {
                let mut current = state.terminals.lock().await;
                if current
                    .get(&key)
                    .is_some_and(|candidate| Arc::ptr_eq(candidate, &bridge))
                {
                    current.remove(&key);
                    true
                } else {
                    false
                }
            };
            if removed {
                bridge.dispose().await;
            }
        }
    }
    drop(terminal_lifecycle);
    let mut subscriptions = subscriptions.lock().await;
    for (_, task) in subscriptions.drain() {
        task.abort();
        let _ = task.await;
    }
    drop(subscriptions);
    // A disconnected requester discards replies, not an in-flight commit or rollback.
    while let Some(result) = mutations.join_next().await {
        if let Err(error) = result {
            eprintln!("WebSocket mutation cleanup: {error}");
        }
    }
}

async fn profile_rpc(
    state: &Host,
    method: &str,
    params: &serde_json::Value,
) -> Result<serde_json::Value, shprd_connections::Error> {
    let profiles = state.profiles.as_ref().ok_or_else(|| {
        shprd_connections::Error::Invalid("connection profiles unavailable".into())
    })?;
    let manager = state.manager.as_ref().ok_or_else(|| {
        shprd_connections::Error::Invalid("connection manager unavailable".into())
    })?;
    let id = || {
        ConnectionId::parse(
            params
                .get("id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(""),
        )
    };
    let profile = || Profile::from_value(params.get("profile").unwrap_or(params).clone());
    match method {
        "connections.list" | "bridge.status" => Ok(
            json!({"default_connection_id":manager.default_id()?,"connections":profiles.read().await.list()?}),
        ),
        "connections.create" => profiles.write().await.create(profile()?).await,
        "connections.update" => {
            profiles
                .write()
                .await
                .update(&id()?, profile()?, connections::probe)
                .await
        }
        "connections.remove" => profiles.write().await.remove(&id()?).await,
        "connections.set_default" => profiles.write().await.set_default(&id()?),
        "connections.connect" => {
            let id = id()?;
            manager.connect(&id).await?;
            profiles.read().await.item(&id)
        }
        "connections.disconnect" => {
            let id = id()?;
            manager.disconnect(&id).await?;
            profiles.read().await.item(&id)
        }
        "connections.test" => Ok(serde_json::to_value(
            profiles
                .read()
                .await
                .test(&profile()?, connections::probe)
                .await?,
        )?),
        _ => Err(shprd_connections::Error::Invalid(
            "unknown bridge method".into(),
        )),
    }
}

async fn rpc(
    state: &Host,
    request: &serde_json::Value,
    subscriptions: &Mutex<HashMap<String, JoinHandle<()>>>,
    events: &mpsc::Sender<Outgoing>,
    outgoing_lease: &mut Option<shprd_connections::Lease>,
    viewer_id: &str,
    terminal_events: &mpsc::UnboundedSender<crate::terminal_bridge::TerminalEvent>,
) -> serde_json::Value {
    let id = request.get("id").and_then(serde_json::Value::as_str);
    let Some(method) = request
        .get("method")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
    else {
        return json!({"id":id,"error":{"message":"missing id/method"}});
    };
    if id.is_none_or(str::is_empty) {
        return json!({"id":id,"error":{"message":"missing id/method"}});
    }
    let global = method.starts_with("bridge.") || method.starts_with("connections.");
    if global
        && (request.get("connection_id").is_some()
            || request.get("connection_generation").is_some())
    {
        return json!({"id":id,"error":{"message":"bridge-global method must not include connection identity"}});
    }
    if method == "bridge.ping" {
        return json!({"id":id,"result":{"ok":true}});
    }
    if method.starts_with("terminal.") {
        return terminal_rpc(
            state,
            request,
            method,
            id,
            viewer_id,
            terminal_events,
            outgoing_lease,
        )
        .await;
    }
    if method == "connections.list" || method == "bridge.status" {
        if state.profiles.is_some() {
            return match profile_rpc(state, method, &json!({})).await {
                Ok(result) => json!({"id":id,"result":result}),
                Err(error) => {
                    json!({"id":id,"error":{"message":shprd_connections::sanitize_error(&error.to_string())}})
                }
            };
        }
        let ping = herdr::call(&state.socket, "ping", &json!({}), Duration::from_secs(8)).await;
        let mut connection = json!({"id":"legacy-default","label":"Default","source":"legacy-config","is_default":true,"generation":1,"read_only":true,"auto_connect":true});
        match ping {
            Ok(info)
                if info
                    .get("protocol")
                    .and_then(serde_json::Value::as_u64)
                    .is_some_and(|p| (14..=20).contains(&p) || p == 22) =>
            {
                connection["state"] = json!("ready")
            }
            Ok(_) => {
                connection["state"] = json!("error");
                connection["error"] = json!({"message":"unsupported Herdr protocol"});
            }
            Err(error) => {
                connection["state"] = json!("error");
                connection["error"] = json!({"message":error.to_string()});
            }
        }
        return json!({"id":id,"result":{"default_connection_id":"legacy-default","connections":[connection]}});
    }
    if let Some(value) = request.get("params") {
        if !value.is_object() {
            return json!({"id":id,"error":{"message":"invalid params"}});
        }
    }
    if global {
        if state.profiles.is_some() {
            return match profile_rpc(state, method, request.get("params").unwrap_or(&json!({})))
                .await
            {
                Ok(result) => json!({"id":id,"result":result}),
                Err(error) => {
                    json!({"id":id,"error":{"message":shprd_connections::sanitize_error(&error.to_string())}})
                }
            };
        }
        return json!({"id":id,"error":{"message":"unknown bridge method"}});
    }
    if let Some(manager) = &state.manager {
        let lease = match resolve_rpc(manager, request) {
            Ok(RpcRoute::Connection(lease)) => lease,
            Ok(RpcRoute::Bridge) => {
                return json!({"id":id,"error":{"message":"unknown bridge method"}});
            }
            Err(error) => {
                return json!({"id":id,"connection_id":request.get("connection_id"),"error":{"message":shprd_connections::sanitize_error(&error.to_string())}});
            }
        };
        let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
        *outgoing_lease = Some(lease.clone());
        let result = if method.starts_with("agent_control.") {
            agent_control(state, method, &params, subscriptions, events, Some(&lease)).await
        } else {
            herdr::call(
                &lease.paths.control,
                method,
                &params,
                Duration::from_secs(8),
            )
            .await
            .map_err(|error| error.to_string())
        };
        let result = if lease.is_current() {
            result
        } else {
            Err("connection changed during request".into())
        };
        return match result {
            Ok(result) => {
                json!({"id":id,"connection_id":lease.connection_id,"connection_generation":lease.generation(),"result":result})
            }
            Err(error) => {
                json!({"id":id,"connection_id":lease.connection_id,"connection_generation":lease.generation(),"error":{"message":shprd_connections::sanitize_error(&error)}})
            }
        };
    }
    if request
        .get("connection_id")
        .is_some_and(|value| value.as_str() != Some("legacy-default"))
    {
        return json!({"id":id,"error":{"message":"unknown connection"}});
    }
    if request
        .get("connection_generation")
        .is_some_and(|value| value.as_u64() != Some(1))
    {
        return json!({"id":id,"connection_id":"legacy-default","error":{"message":"connection generation changed"}});
    }
    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
    if method.starts_with("agent_control.") {
        let result = agent_control(state, method, &params, subscriptions, events, None).await;
        return match result {
            Ok(result) => {
                json!({"id":id,"connection_id":"legacy-default","connection_generation":1,"result":result})
            }
            Err(error) => {
                json!({"id":id,"connection_id":"legacy-default","connection_generation":1,"error":{"message":error}})
            }
        };
    }
    match herdr::call(&state.socket, method, &params, Duration::from_secs(8)).await {
        Ok(result) => {
            json!({"id":id,"connection_id":"legacy-default","connection_generation":1,"result":result})
        }
        Err(error) => {
            json!({"id":id,"connection_id":"legacy-default","connection_generation":1,"error":{"message":error.to_string()}})
        }
    }
}

async fn endpoint_for_terminal(
    lease: Option<&shprd_connections::Lease>,
    cols: u16,
    rows: u16,
) -> Result<Option<crate::endpoint::Endpoint>, String> {
    let Some(lease) = lease else {
        return Ok(None);
    };
    crate::endpoint::Endpoint::connect(&lease.paths.render, cols, rows)
        .await
        .map(Some)
        .map_err(|error| error.to_string())
}

async fn terminal_rpc(
    state: &Host,
    request: &serde_json::Value,
    method: &str,
    id: Option<&str>,
    viewer_id: &str,
    terminal_events: &mpsc::UnboundedSender<crate::terminal_bridge::TerminalEvent>,
    outgoing_lease: &mut Option<shprd_connections::Lease>,
) -> serde_json::Value {
    match tokio::time::timeout(
        Duration::from_secs(8),
        terminal_request(
            state,
            request,
            method,
            id,
            viewer_id,
            terminal_events,
            outgoing_lease,
        ),
    )
    .await
    {
        Ok(response) => response,
        Err(_) => terminal_error(id, "terminal request timed out", outgoing_lease.as_ref()),
    }
}

async fn terminal_request(
    state: &Host,
    request: &Value,
    method: &str,
    id: Option<&str>,
    viewer_id: &str,
    terminal_events: &mpsc::UnboundedSender<crate::terminal_bridge::TerminalEvent>,
    outgoing_lease: &mut Option<shprd_connections::Lease>,
) -> Value {
    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
    let Some(params) = params.as_object() else {
        return terminal_error(id, "invalid params", outgoing_lease.as_ref());
    };
    let _terminal_lifecycle = match method {
        "terminal.attach" | "terminal.detach" => {
            let lifecycle = state.terminal_lifecycle.lock().await;
            // Transport tasks also stop autonomously on lease cancellation.
            let retired = {
                let mut bridges = state.terminals.lock().await;
                let mut retired = Vec::new();
                bridges.retain(|_, bridge| {
                    if bridge.is_current() {
                        true
                    } else {
                        retired.push(Arc::clone(bridge));
                        false
                    }
                });
                retired
            };
            for bridge in retired {
                bridge.dispose().await;
            }
            Some(lifecycle)
        }
        _ => None,
    };
    let terminal_id = match params.get("terminal_id") {
        None => None,
        Some(value) => match value
            .as_str()
            .filter(|id| !id.is_empty() && id.len() <= 256)
        {
            Some(id) => Some(id),
            None => return terminal_error(id, "invalid terminal_id", outgoing_lease.as_ref()),
        },
    };
    let pane_id = match params.get("pane_id") {
        None => None,
        Some(value) => match value
            .as_str()
            .filter(|id| !id.is_empty() && id.len() <= 256)
        {
            Some(id) => Some(id),
            None => return terminal_error(id, "invalid pane_id", outgoing_lease.as_ref()),
        },
    };
    let (render_socket, bridge_lease) = if let Some(manager) = &state.manager {
        let lease = match resolve_rpc(manager, request) {
            Ok(RpcRoute::Connection(lease)) => lease,
            Ok(RpcRoute::Bridge) => {
                return terminal_error(id, "unknown connection", outgoing_lease.as_ref());
            }
            Err(error) => {
                return terminal_error(
                    id,
                    &shprd_connections::sanitize_error(&error.to_string()),
                    outgoing_lease.as_ref(),
                );
            }
        };
        *outgoing_lease = Some(lease.clone());
        (lease.paths.render.clone(), Some(lease))
    } else {
        if request
            .get("connection_id")
            .is_some_and(|value| value.as_str() != Some("legacy-default"))
        {
            return terminal_error(id, "unknown connection", None);
        }
        if request
            .get("connection_generation")
            .is_some_and(|value| value.as_u64() != Some(1))
        {
            return terminal_error(id, "connection generation changed", None);
        }
        (state.socket.clone(), None)
    };
    let operation = async {
        let bridge_key = format!(
            "{}:{}:{}",
            bridge_lease
                .as_ref()
                .map_or("legacy-default", |lease| lease.connection_id.as_str()),
            bridge_lease
                .as_ref()
                .map_or(1, shprd_connections::Lease::generation),
            render_socket.to_string_lossy()
        );
        let bridge = {
            let mut bridges = state.terminals.lock().await;
            if let Some(bridge) = bridges.get(&bridge_key) {
                Arc::clone(bridge)
            } else {
                let bridge = Arc::new(TerminalBridge::with_lease(render_socket, bridge_lease));
                bridges.insert(bridge_key.clone(), Arc::clone(&bridge));
                bridge
            }
        };
        let result = match method {
            "terminal.attach" => {
                let Some(terminal_id) = terminal_id else {
                    return terminal_error(id, "terminal_id required", outgoing_lease.as_ref());
                };
                let cols = match params.get("cols") {
                    None => 100,
                    Some(value) => match value.as_u64() {
                        Some(value) => value,
                        None => return terminal_error(id, "invalid cols", outgoing_lease.as_ref()),
                    },
                };
                let rows = match params.get("rows") {
                    None => 30,
                    Some(value) => match value.as_u64() {
                        Some(value) => value,
                        None => return terminal_error(id, "invalid rows", outgoing_lease.as_ref()),
                    },
                };
                if !(1..=65_535).contains(&cols) || !(1..=65_535).contains(&rows) {
                    return terminal_error(
                        id,
                        "valid terminal cols and rows required",
                        outgoing_lease.as_ref(),
                    );
                }
                let surface_cols = match params.get("surface_cols") {
                    None => None,
                    Some(value) => match value.as_u64() {
                        Some(value) => Some(value),
                        None => {
                            return terminal_error(
                                id,
                                "invalid surface_cols",
                                outgoing_lease.as_ref(),
                            );
                        }
                    },
                };
                let surface_rows = match params.get("surface_rows") {
                    None => None,
                    Some(value) => match value.as_u64() {
                        Some(value) => Some(value),
                        None => {
                            return terminal_error(
                                id,
                                "invalid surface_rows",
                                outgoing_lease.as_ref(),
                            );
                        }
                    },
                };
                match (surface_cols, surface_rows) {
                    (None, None) => {}
                    (Some(cols), Some(rows))
                        if (1..=65_535).contains(&cols) && (1..=65_535).contains(&rows) => {}
                    _ => {
                        return terminal_error(
                            id,
                            "surface_cols and surface_rows must be integers between 1 and 65535",
                            outgoing_lease.as_ref(),
                        );
                    }
                }
                let transport = if bridge.has_terminal(terminal_id).await {
                    crate::terminal_bridge::Transport::Direct(20)
                } else if let Some(pane_id) = pane_id {
                    match endpoint_for_terminal(
                        outgoing_lease.as_ref(),
                        surface_cols.unwrap_or(cols).try_into().unwrap_or(u16::MAX),
                        surface_rows.unwrap_or(rows).try_into().unwrap_or(u16::MAX),
                    )
                    .await
                    {
                        Ok(Some(endpoint)) => crate::terminal_bridge::Transport::Endpoint {
                            pane_id: pane_id.to_owned(),
                            endpoint: Box::new(endpoint),
                        },
                        Ok(None) => crate::terminal_bridge::Transport::Direct(20),
                        Err(error) => return terminal_error(id, &error, outgoing_lease.as_ref()),
                    }
                } else {
                    let protocol = if let Some(lease) = outgoing_lease.as_ref() {
                        match herdr::call(
                            &lease.paths.control,
                            "ping",
                            &json!({}),
                            Duration::from_secs(8),
                        )
                        .await
                        {
                            Ok(info) => match info.get("protocol").and_then(Value::as_u64) {
                                Some(protocol @ (14..=20 | 22)) => protocol as u32,
                                _ => {
                                    return terminal_error(
                                        id,
                                        "unsupported Herdr protocol",
                                        outgoing_lease.as_ref(),
                                    );
                                }
                            },
                            Err(error) => {
                                return terminal_error(
                                    id,
                                    &error.to_string(),
                                    outgoing_lease.as_ref(),
                                );
                            }
                        }
                    } else {
                        20
                    };
                    crate::terminal_bridge::Transport::Direct(protocol)
                };
                bridge
                    .attach(crate::terminal_bridge::AttachRequest {
                        viewer_id: viewer_id.to_owned(),
                        terminal_id: terminal_id.to_owned(),
                        cols,
                        rows,
                        transport,
                        surface_cols,
                        surface_rows,
                        sender: terminal_events.clone(),
                    })
                    .await
            }
            "terminal.input" => {
                let Some(data) = params.get("data").and_then(Value::as_str) else {
                    return terminal_error(id, "data required", outgoing_lease.as_ref());
                };
                bridge
                    .input(viewer_id, terminal_id.unwrap_or(""), data, now_ms())
                    .await
                    .map(|()| json!({"ok":true}))
            }
            "terminal.resize" => bridge
                .resize(
                    viewer_id,
                    terminal_id.unwrap_or(""),
                    match params.get("cols").and_then(Value::as_u64) {
                        Some(value) => value,
                        None => return terminal_error(id, "invalid cols", outgoing_lease.as_ref()),
                    },
                    match params.get("rows").and_then(Value::as_u64) {
                        Some(value) => value,
                        None => return terminal_error(id, "invalid rows", outgoing_lease.as_ref()),
                    },
                )
                .await
                .map(|()| json!({"ok":true})),
            "terminal.scroll" => bridge
                .scroll(crate::terminal_bridge::ScrollRequest {
                    viewer_id: viewer_id.to_owned(),
                    terminal_id: terminal_id.unwrap_or("").to_owned(),
                    direction: params
                        .get("direction")
                        .and_then(Value::as_str)
                        .unwrap_or("down")
                        .to_owned(),
                    lines: match params.get("lines").and_then(Value::as_u64) {
                        Some(value) => value,
                        None => {
                            return terminal_error(id, "invalid lines", outgoing_lease.as_ref());
                        }
                    },
                    column: match params.get("column") {
                        None => None,
                        Some(value) => match value.as_u64() {
                            Some(value) => Some(value),
                            None => {
                                return terminal_error(
                                    id,
                                    "invalid column",
                                    outgoing_lease.as_ref(),
                                );
                            }
                        },
                    },
                    row: match params.get("row") {
                        None => None,
                        Some(value) => match value.as_u64() {
                            Some(value) => Some(value),
                            None => {
                                return terminal_error(id, "invalid row", outgoing_lease.as_ref());
                            }
                        },
                    },
                    source: params
                        .get("source")
                        .and_then(Value::as_str)
                        .unwrap_or("wheel")
                        .to_owned(),
                })
                .await
                .map(|()| json!({"ok":true})),
            "terminal.detach" => {
                if bridge.detach_and_is_empty(viewer_id, terminal_id).await {
                    let removed = {
                        let mut bridges = state.terminals.lock().await;
                        if bridges
                            .get(&bridge_key)
                            .is_some_and(|current| Arc::ptr_eq(current, &bridge))
                        {
                            bridges.remove(&bridge_key);
                            true
                        } else {
                            false
                        }
                    };
                    if removed {
                        bridge.dispose().await;
                    }
                }
                Ok(json!({"ok":true}))
            }
            _ => Err(crate::terminal_bridge::Error::NotAttached),
        };
        let payload = match result {
            Ok(result) => json!({"id":id,"result":result}),
            Err(error) => json!({"id":id,"error":{"message":error.to_string()}}),
        };
        terminal_response_payload(id, payload, outgoing_lease.as_ref())
    };
    tokio::select! {
        biased;
        _ = async { match outgoing_lease.as_ref() {
            Some(lease) => lease.cancelled().await,
            None => std::future::pending().await,
        }} => terminal_error(id, "connection changed during request", outgoing_lease.as_ref()),
        result = operation => result,
    }
}

fn terminal_error(
    id: Option<&str>,
    message: &str,
    lease: Option<&shprd_connections::Lease>,
) -> Value {
    terminal_response_payload(id, json!({"id":id,"error":{"message":message}}), lease)
}

fn terminal_response_payload(
    id: Option<&str>,
    payload: Value,
    lease: Option<&shprd_connections::Lease>,
) -> Value {
    if let Some(lease) = lease {
        if !lease.is_current() {
            return json!({
                "id": id,
                "connection_id": lease.connection_id,
                "connection_generation": lease.generation(),
                "error": {"message": "connection changed during request"},
            });
        }
        return match payload.as_object() {
            Some(payload) if payload.contains_key("result") => json!({
                "id": id,
                "connection_id": lease.connection_id,
                "connection_generation": lease.generation(),
                "result": payload.get("result"),
            }),
            Some(payload) => json!({
                "id": id,
                "connection_id": lease.connection_id,
                "connection_generation": lease.generation(),
                "error": payload.get("error"),
            }),
            None => payload,
        };
    }
    let mut payload = payload;
    payload["id"] = json!(id);
    payload["connection_id"] = json!("legacy-default");
    payload["connection_generation"] = json!(1);
    payload
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            duration.as_millis().try_into().unwrap_or(u64::MAX)
        })
}

async fn agent_control(
    state: &Host,
    method: &str,
    params: &serde_json::Value,
    subscriptions: &Mutex<HashMap<String, JoinHandle<()>>>,
    events: &mpsc::Sender<Outgoing>,
    lease: Option<&shprd_connections::Lease>,
) -> Result<serde_json::Value, String> {
    if let (Some(lease), Some(profiles)) = (lease, &state.profiles) {
        let profiles = profiles.read().await;
        let profile = profiles
            .profile(&lease.connection_id)
            .map_err(|error| error.to_string())?;
        if !matches!(
            profile.transport(),
            shprd_connections::Transport::Local { .. }
        ) {
            return Err("agent attachments are unavailable for remote connections".into());
        }
        lease.check().map_err(|error| error.to_string())?;
    }
    let directory = state
        .attachments
        .as_ref()
        .ok_or("agent attachments are unavailable")?;
    if method == "agent_control.list" {
        return shprd_agent::list(directory)
            .await
            .map_err(|error| error.to_string());
    }
    let id = params
        .get("session_id")
        .and_then(serde_json::Value::as_str)
        .filter(|id| !id.is_empty() && id.len() <= 256)
        .ok_or("invalid agent session_id")?;
    let connection_id = lease
        .map(|lease| lease.connection_id.as_str())
        .unwrap_or("legacy-default");
    let generation = lease.map(shprd_connections::Lease::generation).unwrap_or(1);
    let key = format!("{connection_id}/{generation}/{id}");
    match method {
        "agent_control.request" => {
            let command: shprd_agent::Command = serde_json::from_value(
                params
                    .get("command")
                    .cloned()
                    .ok_or("missing agent command")?,
            )
            .map_err(|_| "invalid agent command")?;
            shprd_agent::request(directory, id, &command, |_| {})
                .await
                .map_err(|error| error.to_string())
        }
        "agent_control.subscribe" => {
            let mut subscriptions = subscriptions.lock().await;
            subscriptions.retain(|_, task| !task.is_finished());
            if subscriptions
                .get(&key)
                .is_some_and(|task| !task.is_finished())
            {
                return Ok(json!({"ok":true}));
            }
            if subscriptions.len() >= 16 && !subscriptions.contains_key(&key) {
                return Err("too many agent subscriptions".into());
            }
            let mut attachment = shprd_agent::Attachment::connect(directory, id)
                .await
                .map_err(|error| error.to_string())?;
            let events = events.clone();
            let session_id = id.to_owned();
            let connection_id = connection_id.to_owned();
            let lease = lease.cloned();
            let task = tokio::spawn(async move {
                let cancelled = async {
                    match &lease {
                        Some(lease) => lease.cancelled().await,
                        None => std::future::pending().await,
                    }
                };
                tokio::pin!(cancelled);
                loop {
                    let next = tokio::select! {
                        biased;
                        _ = &mut cancelled => break,
                        event = attachment.next_event() => event,
                    };
                    let (data, lost) = match next {
                        Ok(data) => (data, false),
                        Err(_) => (
                            json!({"agent_event":{"session_id":session_id,"event":{"type":"attachment_lost"}}}),
                            true,
                        ),
                    };
                    if lease.as_ref().is_some_and(|lease| !lease.is_current()) {
                        break;
                    }
                    let outgoing = Outgoing {
                        payload: json!({"connection_id":connection_id,"connection_generation":generation,"event":"agent_control.event","data":data}),
                        lease: lease.clone(),
                    };
                    let sent = tokio::select! {
                        biased;
                        _ = &mut cancelled => break,
                        sent = events.send(outgoing) => sent,
                    };
                    if sent.is_err() || lost {
                        break;
                    }
                }
            });
            subscriptions.insert(key, task);
            Ok(json!({"ok":true}))
        }
        "agent_control.unsubscribe" => {
            if let Some(task) = subscriptions.lock().await.remove(&key) {
                task.abort();
                let _ = task.await;
            }
            Ok(json!({"ok":true}))
        }
        _ => Err("unknown agent control method".into()),
    }
}

const LOGIN: &str = r#"<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>SHPRD login</title><style>body{font:16px system-ui;background:#0f1115;color:#e6e8ee;display:grid;place-items:center;min-height:100dvh;margin:0}form{width:min(320px,85vw)}input,button{box-sizing:border-box;width:100%;padding:12px;margin-top:12px}p{min-height:24px}</style><form><h1>SHPRD</h1><label for="password">Password or token</label><input id="password" type="password" autocomplete="current-password" required><button>Log in</button><p role="alert"></p></form><script>document.querySelector('form').addEventListener('submit',async event=>{event.preventDefault();const error=document.querySelector('[role=alert]');try{const response=await fetch('/api/login',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({password:document.querySelector('input').value})});if(response.ok){location.href='/'}else{error.textContent='Wrong password or token'}}catch{error.textContent='Connection failed. Try again.'}})</script></html>"#;

#[cfg(unix)]
#[tokio::test]
async fn terminal_host_serializes_last_detach_against_concurrent_attach()
-> Result<(), Box<dyn std::error::Error>> {
    use futures_util::{SinkExt, StreamExt};
    use tokio::net::UnixListener;
    use tokio_util::codec::LengthDelimitedCodec;

    let directory = tempfile::tempdir()?;
    let render_path = directory.path().join("render.sock");
    let listener = UnixListener::bind(&render_path)?;
    let (closed, mut closed_events) = mpsc::unbounded_channel();
    let fixture = tokio::spawn(async move {
        let mut connections = 0_u8;
        loop {
            let (socket, _) = listener.accept().await?;
            connections += 1;
            let closed = closed.clone();
            tokio::spawn(async move {
                let mut wire = LengthDelimitedCodec::builder()
                    .little_endian()
                    .new_framed(socket);
                let hello = wire.next().await.ok_or("missing terminal hello")??;
                assert_eq!(hello.as_ref(), &[0, 20, 80, 24, 0, 0, 1, 0, 2]);
                wire.send(vec![0, 20, 1, 0].into()).await?;
                let attach = wire.next().await.ok_or("missing terminal attach")??;
                let ((tag, terminal_id, takeover), _): ((u32, String, bool), usize) =
                    bincode::decode_from_slice(&attach, bincode::config::standard())?;
                assert_eq!(
                    (tag, terminal_id.as_str(), takeover),
                    (5, "term-race", true)
                );
                while wire.next().await.transpose()?.is_some() {}
                closed.send(()).map_err(|_| "closed observer dropped")?;
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
            });
            if connections == 2 {
                break;
            }
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(connections)
    });
    let host = Arc::new(Host {
        socket: render_path,
        auth: Auth::new(false, String::new())?,
        attachments: None,
        profiles: None,
        manager: None,
        terminals: Mutex::new(HashMap::new()),
        terminal_lifecycle: Mutex::new(()),
    });
    let (terminal_events, _) = mpsc::unbounded_channel();
    let request = |id: &str, method: &str, viewer: &str| {
        json!({
            "id": id,
            "method": method,
            "params": {"terminal_id":"term-race","cols":80,"rows":24,"viewer":viewer}
        })
    };
    let attach_a = request("attach-a", "terminal.attach", "viewer-a");
    let mut lease_a = None;
    let response = terminal_rpc(
        &host,
        &attach_a,
        "terminal.attach",
        Some("attach-a"),
        "viewer-a",
        &terminal_events,
        &mut lease_a,
    )
    .await;
    assert_eq!(response["result"], json!({"ok":true}));
    assert_eq!(host.terminals.lock().await.len(), 1);

    let detach = request("detach-a", "terminal.detach", "viewer-a");
    let attach_b = request("attach-b", "terminal.attach", "viewer-b");
    let detach_host = Arc::clone(&host);
    let attach_host = Arc::clone(&host);
    let detach_events = terminal_events.clone();
    let attach_events = terminal_events.clone();
    let detach_task = tokio::spawn(async move {
        let mut lease = None;
        terminal_rpc(
            &detach_host,
            &detach,
            "terminal.detach",
            Some("detach-a"),
            "viewer-a",
            &detach_events,
            &mut lease,
        )
        .await
    });
    let attach_task = tokio::spawn(async move {
        let mut lease = None;
        terminal_rpc(
            &attach_host,
            &attach_b,
            "terminal.attach",
            Some("attach-b"),
            "viewer-b",
            &attach_events,
            &mut lease,
        )
        .await
    });
    let (detach_response, attach_response) = tokio::try_join!(detach_task, attach_task)?;
    assert_eq!(detach_response["result"], json!({"ok":true}));
    assert_eq!(attach_response["result"], json!({"ok":true}));
    assert_eq!(host.terminals.lock().await.len(), 1);

    let mut input = request("input-b", "terminal.input", "viewer-b");
    input["params"]["data"] = json!("ZWNobyByYWNl");
    let mut lease = None;
    let input = terminal_rpc(
        &host,
        &input,
        "terminal.input",
        Some("input-b"),
        "viewer-b",
        &terminal_events,
        &mut lease,
    )
    .await;
    assert_eq!(input["result"], json!({"ok":true}));

    let detach_b = request("detach-b", "terminal.detach", "viewer-b");
    let mut lease = None;
    let response = terminal_rpc(
        &host,
        &detach_b,
        "terminal.detach",
        Some("detach-b"),
        "viewer-b",
        &terminal_events,
        &mut lease,
    )
    .await;
    assert_eq!(response["result"], json!({"ok":true}));
    assert_eq!(host.terminals.lock().await.len(), 0);
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(1), closed_events.recv())
            .await?
            .is_some()
    );
    fixture.abort();
    let _ = fixture.await;
    Ok(())
}
