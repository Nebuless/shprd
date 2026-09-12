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
