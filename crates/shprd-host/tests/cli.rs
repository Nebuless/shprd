use std::process::Command;

#[test]
fn version_works_without_herdr_or_a_browser() -> Result<(), Box<dyn std::error::Error>> {
    // Given the independently installed native binary.
    // When its version is requested, then no downstream process is required.
    let output = Command::new(env!("CARGO_BIN_EXE_shprd"))
        .arg("--version")
        .output()?;
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout)?.trim(), "shprd 0.6.2");
    Ok(())
}

#[tokio::test]
async fn native_server_serves_health_without_bun() -> Result<(), Box<dyn std::error::Error>> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    let home = tempfile::tempdir()?;
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_shprd"));
    for variable in [
        "HOST",
        "PORT",
        "HERDR_GUI_PASSWORD",
        "HERDR_SESSION",
        "HERDR_SOCKET_PATH",
        "HERDR_CLIENT_SOCKET_PATH",
        "OPEN_BROWSER",
    ] {
        command.env_remove(variable);
    }
    let mut process = command
        .env("HOME", home.path())
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            "0",
            "--socket-path",
            "/missing/control.sock",
        ])
        .kill_on_drop(true)
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let mut startup = BufReader::new(process.stderr.take().ok_or("missing startup output")?);
        let mut line = String::new();
        startup.read_line(&mut line).await?;
        let address = line
            .trim()
            .strip_prefix("SHPRD_LISTENING=http://")
            .ok_or("listener address missing")?;
        let mut socket = tokio::net::TcpStream::connect(address).await?;
        socket
            .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await?;
        let mut response = String::new();
        socket.read_to_string(&mut response).await?;
        assert!(response.starts_with("HTTP/1.1 200"));
        assert!(response.ends_with("Ok"));
        Ok::<_, Box<dyn std::error::Error>>(())
    })
    .await;
    process.kill().await?;
    process.wait().await?;
    result?
}
