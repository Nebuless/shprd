#![cfg(unix)]
use futures_util::{SinkExt, StreamExt};
use shprd_host::terminal::{Output, Terminal};
use tokio::net::UnixListener;
use tokio_util::codec::LengthDelimitedCodec;

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
