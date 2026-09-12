#![cfg(unix)]

use serde_json::{Value, json};
use shprd_host::herdr;
use std::time::Duration;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::UnixListener,
};

#[tokio::test]
async fn rejects_invalid_envelopes_and_oversized_responses()
-> Result<(), Box<dyn std::error::Error>> {
    for reply in [
        "{\"id\":\"wrong\",\"result\":{}}\n".to_owned(),
        "{\"id\":\"rpc\",\"result\":{},\"error\":{}}\n".to_owned(),
        "x".repeat(1024 * 1024 + 1),
    ] {
        // Given an invalid response from a real socket.
        let home = tempfile::tempdir()?;
        let path = home.path().join("control.sock");
        let listener = UnixListener::bind(&path)?;
        let server = async {
            let (socket, _) = listener.accept().await?;
            let mut reader = BufReader::new(socket);
            let mut request = String::new();
            reader.read_line(&mut request).await?;
            reader.get_mut().write_all(reply.as_bytes()).await?;
            Ok::<_, std::io::Error>(())
        };
        // When the client reads it, then the boundary rejects it.
        let client = async {
            assert!(matches!(
                herdr::call(&path, "ping", &json!({}), Duration::from_secs(2)).await,
                Err(herdr::Error::Envelope | herdr::Error::LineTooLarge)
            ));
        };
        let (_, result) = tokio::time::timeout(Duration::from_secs(3), async {
            tokio::join!(client, server)
        })
        .await?;
        result?;
    }
    Ok(())
}

#[tokio::test]
async fn streams_events_only_after_valid_subscription_ack() -> Result<(), Box<dyn std::error::Error>>
{
    // Given a subscription acknowledgement followed by a fragmented event.
    let home = tempfile::tempdir()?;
    let path = home.path().join("control.sock");
    let listener = UnixListener::bind(&path)?;
    let server = async {
        let (socket, _) = listener.accept().await?;
        let mut reader = BufReader::new(socket);
        let mut line = String::new();
        reader.read_line(&mut line).await?;
        let request: Value = serde_json::from_str(&line)?;
        assert_eq!(
            request["params"]["subscriptions"],
            json!([{"type":"pane.created"}])
        );
        reader
            .get_mut()
            .write_all(b"{\"id\":\"sub\",\"result\":{}}\n{\"event\":")
            .await?;
        reader
            .get_mut()
            .write_all(b"\"pane.created\",\"data\":{}}\n")
            .await?;
        let mut remaining = Vec::new();
        reader.read_to_end(&mut remaining).await?;
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    // When the subscription reads an event, then it preserves its wire payload.
    let client = async {
        let mut subscription = herdr::Subscription::open(
            &path,
            &json!([{"type":"pane.created"}]),
            Duration::from_secs(2),
        )
        .await?;
        assert_eq!(
            subscription.next().await?,
            json!({"event":"pane.created","data":{}})
        );
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    tokio::time::timeout(Duration::from_secs(3), async {
        tokio::try_join!(client, server)
    })
    .await??;
    Ok(())
}

#[tokio::test]
async fn returns_result_and_closes_one_shot_socket() -> Result<(), Box<dyn std::error::Error>> {
    // Given a real local socket whose peer waits for client EOF.
    let home = tempfile::tempdir()?;
    let path = home.path().join("control.sock");
    let listener = UnixListener::bind(&path)?;
    let server = async {
        let (socket, _) = listener.accept().await?;
        let mut reader = BufReader::new(socket);
        let mut line = String::new();
        reader.read_line(&mut line).await?;
        let request: Value = serde_json::from_str(&line)?;
        assert_eq!(request["method"], "ping");
        reader
            .get_mut()
            .write_all(
                format!(
                    "{}\n",
                    json!({"id":request["id"], "result":{"protocol":22}})
                )
                .as_bytes(),
            )
            .await?;
        let mut remaining = Vec::new();
        reader.read_to_end(&mut remaining).await?;
        assert!(remaining.is_empty());
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    // When a single request completes.
    let client = async {
        let result = herdr::call(&path, "ping", &json!({}), Duration::from_secs(2)).await;
        // Then the result matches the wire response.
        assert_eq!(result?, json!({"protocol":22}));
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    tokio::time::timeout(Duration::from_secs(3), async {
        tokio::try_join!(client, server)
    })
    .await??;
    Ok(())
}
