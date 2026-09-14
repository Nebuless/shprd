use clap::Parser;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

fn command(home: &std::path::Path, isolated: &std::path::Path, password: bool) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_shprd"));
    for variable in [
        "HOST",
        "PORT",
        "HERDR_GUI_PASSWORD",
        "HERDR_SESSION",
        "HERDR_SOCKET_PATH",
        "HERDR_CLIENT_SOCKET_PATH",
        "HERDR_GUI_CONNECTIONS_PATH",
        "SHPRD_CONFIG_DIR",
        "SHPRD_AGENT_DIR",
        "OPEN_BROWSER",
    ] {
        command.env_remove(variable);
    }
    command
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("APPDATA", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("SHPRD_CONFIG_DIR", isolated)
        .args(["--host", "0.0.0.0", "--port", "0"])
        .args(
            password
                .then_some(["--password", "native-secret"])
                .into_iter()
                .flatten(),
        )
        .arg("--socket-path")
        .arg(home.join("herdr-control.sock"))
        .arg("--client-socket-path")
        .arg(home.join("herdr-render.sock"))
        .kill_on_drop(true)
        .stderr(Stdio::piped())
        .stdout(Stdio::null());
    command
}

async fn startup(process: &mut Child) -> Result<(String, String), Box<dyn std::error::Error>> {
    let stderr = process.stderr.take().ok_or("missing stderr")?;
    let mut lines = BufReader::new(stderr).lines();
    let mut output = String::new();
    loop {
        let line = tokio::time::timeout(std::time::Duration::from_secs(5), lines.next_line())
            .await??
            .ok_or("native process exited before readiness")?;
        output.push_str(&line);
        output.push('\n');
        if let Some(address) = line.strip_prefix("SHPRD_LISTENING=http://") {
            return Ok((address.to_owned(), output));
        }
    }
}

async fn stop(process: &mut Child) -> Result<(), Box<dyn std::error::Error>> {
    process.kill().await?;
    process.wait().await?;
    Ok(())
}

fn loopback(address: &str) -> Result<String, Box<dyn std::error::Error>> {
    Ok(format!(
        "127.0.0.1:{}",
        address.rsplit_once(':').ok_or("port")?.1
    ))
}

async fn login(address: &str) -> Result<String, Box<dyn std::error::Error>> {
    let address = loopback(address)?;
    let body = br#"{"password":"native-secret"}"#;
    let mut socket = tokio::net::TcpStream::connect(&address).await?;
    socket
        .write_all(
            format!(
                "POST /api/login HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .as_bytes(),
        )
        .await?;
    socket.write_all(body).await?;
    let mut response = String::new();
    socket.read_to_string(&mut response).await?;
    response
        .lines()
        .find_map(|line| {
            line.strip_prefix("set-cookie: ")
                .or_else(|| line.strip_prefix("Set-Cookie: "))
                .map(|cookie| cookie.split(';').next().unwrap_or(cookie).to_owned())
        })
        .ok_or_else(|| format!("missing cookie in response: {response}").into())
}

fn websocket_request(
    address: &str,
    cookie: &str,
) -> Result<tokio_tungstenite::tungstenite::http::Request<()>, Box<dyn std::error::Error>> {
    let mut request = format!("ws://{address}/ws").into_client_request()?;
    request.headers_mut().insert("cookie", cookie.parse()?);
    Ok(request)
}

#[tokio::test]
async fn native_binary_uses_isolated_token_path_and_preserves_sentinels()
-> Result<(), Box<dyn std::error::Error>> {
    // Given a preexisting branded token and unrelated legacy sentinel.
    let directory = tempfile::tempdir()?;
    let home = directory.path().join("home");
    let isolated = directory.path().join("shprd/studio");
    tokio::fs::create_dir_all(&isolated).await?;
    let token_path = isolated.join("auth-token");
    let token = format!("{}\n", "a".repeat(64));
    tokio::fs::write(&token_path, &token).await?;
    let sentinel = directory.path().join("legacy-sentinel");
    tokio::fs::write(&sentinel, b"legacy-untouched").await?;

    // When the actual native binary starts on a protected wildcard listener.
    let mut process = command(&home, &isolated, false).spawn()?;
    let result = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let (_address, output) = startup(&mut process).await?;
        assert!(
            output.contains(&format!(
                "Authentication token file: {}",
                token_path.display()
            )),
            "startup output: {output}"
        );
        assert_eq!(tokio::fs::read_to_string(&token_path).await?, token);
        assert_eq!(
            tokio::fs::read_to_string(&sentinel).await?,
            "legacy-untouched"
        );
        Ok::<_, Box<dyn std::error::Error>>(())
    })
    .await;
    stop(&mut process).await?;
    result??;
    Ok(())
}

#[test]
fn native_config_keeps_explicit_socket_roots_outside_shprd_config()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let isolated = directory.path().join("shprd/studio");
    let control = directory.path().join("herdr-control.sock");
    let render = directory.path().join("herdr-render.sock");
    let args = shprd_host::config::Args::try_parse_from([
        "shprd",
        "--config-dir",
        isolated.to_str().ok_or("isolated path")?,
        "--socket-path",
        control.to_str().ok_or("control path")?,
        "--client-socket-path",
        render.to_str().ok_or("render path")?,
    ])?;
    assert_eq!(
        args.auth_token_path(directory.path()),
        isolated.join("auth-token")
    );
    assert_eq!(args.control_socket(directory.path()), control);
    assert_eq!(args.render_socket(directory.path()), render);
    Ok(())
}

#[tokio::test]
async fn native_binary_refuses_malformed_isolated_token_without_regeneration()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let home = directory.path().join("home");
    let isolated = directory.path().join("shprd/studio");
    tokio::fs::create_dir_all(&isolated).await?;
    let token_path = isolated.join("auth-token");
    let token = "not-a-token\n";
    tokio::fs::write(&token_path, token).await?;
    let legacy_token_path = home.join(".config/herdr-gui/auth-token");
    tokio::fs::create_dir_all(legacy_token_path.parent().ok_or("legacy token parent")?).await?;
    tokio::fs::write(&legacy_token_path, token).await?;

    let mut process = command(&home, &isolated, false).spawn()?;
    let status = tokio::time::timeout(std::time::Duration::from_secs(10), process.wait()).await?;
    assert!(!status?.success());
    assert_eq!(tokio::fs::read_to_string(&legacy_token_path).await?, token);
    assert_eq!(tokio::fs::read_to_string(&token_path).await?, token);
    Ok(())
}

#[tokio::test]
async fn native_binary_uses_shprd_cookie_for_isolated_config()
-> Result<(), Box<dyn std::error::Error>> {
    // Given an isolated branded installation.
    let directory = tempfile::tempdir()?;
    let home = directory.path().join("home");
    let isolated = directory.path().join("shprd/studio");
    tokio::fs::create_dir_all(&isolated).await?;

    // When the actual binary accepts a login over HTTP.
    let mut process = command(&home, &isolated, true).spawn()?;
    let result = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let (address, _output) = startup(&mut process).await?;
        let cookie = login(&address).await?;
        assert!(cookie.starts_with("shprd_auth="), "cookie: {cookie}");
        Ok::<_, Box<dyn std::error::Error>>(())
    })
    .await;
    stop(&mut process).await?;
    result??;
    Ok(())
}

#[tokio::test]
async fn native_binary_loads_isolated_connection_registry() -> Result<(), Box<dyn std::error::Error>>
{
    // Given a branded registry containing a profile absent from legacy storage.
    let directory = tempfile::tempdir()?;
    let home = directory.path().join("home");
    let isolated = directory.path().join("shprd/studio");
    tokio::fs::create_dir_all(&isolated).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&isolated, std::fs::Permissions::from_mode(0o700)).await?;
    }
    let profile = json!({
        "id": "isolated",
        "label": "Isolated",
        "type": "local",
        "auto_connect": false,
        "control_socket_path": directory.path().join("isolated-control.sock"),
        "client_socket_path": directory.path().join("isolated-render.sock")
    });
    let registry_path = isolated.join("connections.json");
    tokio::fs::write(
        &registry_path,
        serde_json::to_vec(&json!({
            "version": 2,
            "default_connection_id": "isolated",
            "profiles": [profile]
        }))?,
    )
    .await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&registry_path, std::fs::Permissions::from_mode(0o600)).await?;
    }

    // When the actual binary serves the profile list over its WebSocket.
    let mut process = command(&home, &isolated, true).spawn()?;
    let result = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let (address, _output) = startup(&mut process).await?;
        let cookie = login(&address).await?;
        let request = websocket_request(&address, &cookie)?;
        let (mut socket, _) = tokio_tungstenite::connect_async(request).await?;
        socket.next().await.ok_or("missing hello")??;
        socket
            .send(Message::Text(
                json!({"id":"profiles","method":"connections.list"})
                    .to_string()
                    .into(),
            ))
            .await?;
        let reply = socket
            .next()
            .await
            .ok_or("missing profiles")??
            .into_text()?;
        assert!(reply.contains("\"id\":\"isolated\""), "reply: {reply}");
        socket.close(None).await?;
        Ok::<_, Box<dyn std::error::Error>>(())
    })
    .await;
    stop(&mut process).await?;
    result??;
    Ok(())
}
