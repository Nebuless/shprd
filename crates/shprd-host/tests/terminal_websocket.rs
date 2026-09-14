#![cfg(unix)]
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use shprd_connections::{
    ConnectionId, Manager, Profile, ProfileService, Runtime, RuntimeContext, RuntimeFuture,
    SocketPaths, Store,
};
use shprd_host::{auth::Auth, host};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream, UnixListener},
    sync::{mpsc, oneshot},
};
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};
use tokio_util::codec::LengthDelimitedCodec;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct Ready(SocketPaths);
impl Runtime for Ready {
    fn start<'a>(&'a self, _: &'a RuntimeContext) -> RuntimeFuture<'a, SocketPaths> {
        Box::pin(async { Ok(self.0.clone()) })
    }
    fn stop(&self) -> RuntimeFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
}

async fn receive<S>(socket: &mut WebSocketStream<S>) -> TestResult<Value>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let message = socket.next().await.ok_or("missing WebSocket message")??;
    assert!(message.is_text(), "terminal envelopes must use JSON text");
    Ok(serde_json::from_str(message.to_text()?)?)
}

#[tokio::test]
async fn native_terminal_websocket_lifecycle() -> TestResult {
    for protocol in [20_u8, 22] {
        let directory = tempfile::tempdir()?;
        let paths = SocketPaths {
            control: directory.path().join("control.sock"),
            render: directory.path().join("render.sock"),
        };
        let control = UnixListener::bind(&paths.control)?;
        let render = UnixListener::bind(&paths.render)?;
        let id = ConnectionId::parse("legacy-default")?;
        let manager = Arc::new(Manager::new(id.clone()));
        let ready = Arc::new(Ready(paths.clone()));
        let profiles = ProfileService::load(
            Store::new(directory.path().join("connections.json"))?,
            Profile::legacy(
                paths.control.to_str().ok_or("path")?,
                paths.render.to_str().ok_or("path")?,
            )?,
            true,
            Arc::clone(&manager),
            Arc::new(move |_| {
                let ready = Arc::clone(&ready);
                Arc::new(move |_| Ok(ready.clone()))
            }),
        )?;
        manager.connect(&id).await?;
        manager.disconnect(&id).await?;
        manager.connect(&id).await?;
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let router = host::configured_router_with_profiles(
            paths.control.clone(),
            directory.path().into(),
            Auth::new(false, String::new())?,
            None,
            profiles,
            Arc::clone(&manager),
        );
        let (stop, stopped) = oneshot::channel();
        let (stop_control, mut control_stopped) = oneshot::channel();
        let (emit, mut emitted) = mpsc::channel(1);
        let (closed, mut disconnected) = mpsc::channel(1);
        let server = async {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await?;
            TestResult::Ok(())
        };
        let control_fixture = async {
            loop {
                let (socket, _) = tokio::select! {
                    _ = &mut control_stopped => break,
                    accepted = control.accept() => accepted?,
                };
                let mut socket = BufReader::new(socket);
                let mut line = String::new();
                socket.read_line(&mut line).await?;
                let request: Value = serde_json::from_str(&line)?;
                let response = if request["method"] == "ping" {
                    json!({"id":request["id"],"result":{"protocol":protocol,"version":"fixture"}})
                } else {
                    json!({"id":request["id"],"error":{"message":"control fixture has no terminal RPC handling"}})
                };
                socket
                    .get_mut()
                    .write_all(format!("{response}\n").as_bytes())
                    .await?;
            }
            TestResult::Ok(())
        };
        let render_fixture = async {
            for _ in 0..3 {
                let (socket, _) = render.accept().await?;
                let mut wire = LengthDelimitedCodec::builder()
                    .little_endian()
                    .new_framed(socket);
                let hello = wire.next().await.ok_or("render hello")??;
                assert_eq!(
                    hello.as_ref(),
                    if protocol == 22 {
                        &[0, 22, 100, 30, 0, 0, 0][..]
                    } else {
                        &[0, 20, 100, 30, 0, 0, 1, 0, 2][..]
                    }
                );
                wire.send(vec![0, protocol, 1, 0].into()).await?;
                assert_eq!(
                    wire.next().await.ok_or("render attach")??.as_ref(),
                    b"\x05\x06term_1\x01"
                );
                emitted.recv().await.ok_or("emit output")?;
                let mut frame = vec![if protocol == 22 { 1 } else { 2 }, 1, 100, 30, 1, 5];
                frame.extend_from_slice(b"hello");
                wire.send(frame.into()).await?;
                assert_eq!(
                    wire.next().await.ok_or("render input")??.as_ref(),
                    b"\x01\x03a\x00\xff"
                );
                assert_eq!(
                    wire.next().await.ok_or("render resize")??.as_ref(),
                    if protocol == 22 {
                        &[3, 120, 40, 0, 0, 0][..]
                    } else {
                        &[3, 120, 40, 0, 0][..]
                    }
                );
                assert!(
                    wire.next().await.is_none(),
                    "retired render stream remained open"
                );
                closed.send(()).await?;
            }
            TestResult::Ok(())
        };
        let client = async {
            let mut http = TcpStream::connect(address).await?;
            http.write_all(
                b"GET /api/health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
            )
            .await?;
            let mut response = String::new();
            http.read_to_string(&mut response).await?;
            assert!(response.starts_with("HTTP/1.1 200"), "{response}");
            let (mut socket, _) =
                tokio_tungstenite::connect_async(format!("ws://{address}/ws")).await?;
            assert_eq!(receive(&mut socket).await?["hello"], true);
            for phase in 0..3 {
                let generation = manager.lease(&id)?.generation();
                assert!(generation > 1);
                let request = |method: &str, params: Value| {
                    Message::Text(json!({"id":method,"method":method,"params":params,"connection_id":id,"connection_generation":generation}).to_string().into())
                };
                socket
                    .send(request(
                        "terminal.attach",
                        json!({"terminal_id":"term_1","cols":100,"rows":30}),
                    ))
                    .await?;
                let attached = receive(&mut socket).await?;
                assert_eq!(
                    attached["result"]["ok"], true,
                    "native terminal.attach failed: {attached}"
                );
                assert_eq!(attached["connection_id"], id.as_str());
                assert_eq!(attached["connection_generation"], generation);
                emit.send(()).await?;
                assert_eq!(
                    receive(&mut socket).await?,
                    json!({"connection_id":id,"connection_generation":generation,"terminal":{"terminal_id":"term_1","width":100,"height":30,"full":true,"bytes":"aGVsbG8="}})
                );
                for (method, params) in [
                    (
                        "terminal.input",
                        json!({"terminal_id":"term_1","data":"YQD/"}),
                    ),
                    (
                        "terminal.resize",
                        json!({"terminal_id":"term_1","cols":120,"rows":40}),
                    ),
                ] {
                    socket.send(request(method, params)).await?;
                    let reply = receive(&mut socket).await?;
                    assert_eq!(reply["result"]["ok"], true, "{reply}");
                }
                match phase {
                    0 => {
                        socket
                            .send(request("terminal.detach", json!({"terminal_id":"term_1"})))
                            .await?;
                        assert_eq!(receive(&mut socket).await?["result"]["ok"], true);
                        socket
                            .send(request(
                                "terminal.input",
                                json!({"terminal_id":"term_1","data":"YQ=="}),
                            ))
                            .await?;
                        assert!(receive(&mut socket).await?.get("error").is_some());
                    }
                    1 => {
                        manager.disconnect(&id).await?;
                        socket
                            .send(request(
                                "terminal.input",
                                json!({"terminal_id":"term_1","data":"YQ=="}),
                            ))
                            .await?;
                        assert!(receive(&mut socket).await?.get("error").is_some());
                    }
                    _ => socket.close(None).await?,
                }
                tokio::time::timeout(Duration::from_secs(1), disconnected.recv())
                    .await?
                    .ok_or("render close observer")?;
                if phase == 1 {
                    manager.connect(&id).await?;
                }
            }
            drop(socket);
            stop.send(()).map_err(|()| "stop host")?;
            stop_control.send(()).map_err(|()| "stop control")?;
            TestResult::Ok(())
        };
        tokio::time::timeout(Duration::from_secs(8), async {
            tokio::try_join!(server, control_fixture, render_fixture, client)
        })
        .await??;
        manager.stop_all().await?;
    }
    Ok(())
}
