use clap::Parser;
use shprd_host::config::Args;

#[test]
fn rejects_invalid_ports_and_parent_session_paths() {
    // Given untrusted command-line input.
    // When parsing invalid ports, then parsing fails before binding.
    assert!(Args::try_parse_from(["shprd", "--port", "65536"]).is_err());
    assert!(Args::try_parse_from(["shprd", "--session", "../other"]).is_err());
}

#[test]
fn shprd_config_dir_owns_native_durable_paths() -> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let isolated = directory.path().join("shprd/studio");
    let args = Args {
        host: "127.0.0.1".to_owned(),
        port: 8787,
        password: None,
        socket_path: None,
        client_socket_path: None,
        config_dir: Some(isolated.clone()),
        connection_registry_path: None,
        session: None,
        public_dir: "web/dist".into(),
        open: false,
    };
    assert_eq!(
        args.auth_token_path(directory.path()),
        isolated.join("auth-token")
    );
    assert_eq!(
        args.settings_path(directory.path()),
        isolated.join("settings.json")
    );
    assert_eq!(
        args.connection_registry_path(directory.path()),
        isolated.join("connections.json")
    );
    assert_eq!(
        args.control_socket(directory.path()),
        shprd_host::config::config_dir(directory.path())
            .join("herdr")
            .join("herdr.sock"),
        "control socket must remain on legacy Herdr root"
    );
    Ok(())
}

#[test]
fn explicit_connection_registry_override_wins_over_shprd_config_dir()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let isolated = directory.path().join("shprd/studio");
    let override_path = directory.path().join("override/connections.json");
    let args = Args {
        host: "127.0.0.1".to_owned(),
        port: 8787,
        password: None,
        socket_path: None,
        client_socket_path: None,
        config_dir: Some(isolated),
        connection_registry_path: Some(override_path.clone()),
        session: None,
        public_dir: "web/dist".into(),
        open: false,
    };
    assert_eq!(
        args.connection_registry_path(directory.path()),
        override_path
    );
    Ok(())
}

#[test]
fn explicit_socket_overrides_named_session() -> Result<(), Box<dyn std::error::Error>> {
    // Given both an explicit socket and a named session.
    let args = Args::try_parse_from([
        "shprd",
        "--socket-path",
        "/tmp/custom.sock",
        "--session",
        "work",
    ])?;
    // When resolving configuration, then explicit routing wins.
    assert_eq!(
        args.control_socket(std::path::Path::new("/tmp/home")),
        std::path::PathBuf::from("/tmp/custom.sock")
    );
    Ok(())
}

#[test]
fn windows_paths_use_roaming_config_and_named_pipes() {
    // Given Windows configuration roots and filesystem-shaped socket paths.
    // When resolving native paths, then both sockets use named pipes.
    assert_eq!(
        shprd_host::config::native_socket_path(
            "C:\\Users\\tester\\AppData\\Roaming\\herdr\\herdr.sock",
            true
        ),
        "\\\\.\\pipe\\C:\\Users\\tester\\AppData\\Roaming\\herdr\\herdr.sock"
    );
    assert_eq!(
        shprd_host::config::native_socket_path("\\\\.\\pipe\\herdr", true),
        "\\\\.\\pipe\\herdr"
    );
}
