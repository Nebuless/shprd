use serde_json::json;
use shprd_connections::{Profile, Registry, Store};
fn profile() -> serde_json::Value {
    json!({"id":"alpha","label":"Alpha","type":"local","control_socket_path":"/tmp/control","client_socket_path":"/tmp/render","auto_connect":true})
}
fn registry() -> Registry {
    Registry::parse(
        json!({"version":2,"default_connection_id":"alpha","profiles":[profile()]})
            .to_string()
            .as_bytes(),
    )
    .unwrap()
}
#[test]
fn strict_profile_and_registry_boundaries() {
    for (key, value) in [
        ("label", json!("bad\nlabel")),
        ("control_socket_path", json!("/tmp/../secret")),
        ("password", json!("secret")),
        ("auto_connect", json!(1)),
        ("id", json!("bad/id")),
    ] {
        let mut p = profile();
        p[key] = value;
        assert!(Profile::from_value(p).is_err(), "{key}");
    }
    let mut r = registry();
    r.profiles.push(r.profiles[0].clone());
    assert!(r.validate().is_err());
    let ssh = json!({"id":"ssh","label":"SSH","type":"ssh","ssh_destination":"user@host","remote_control_socket_path":"","remote_client_socket_path":"","auto_connect":false});
    assert!(Profile::from_value(ssh.clone()).is_ok());
    for bad in [
        "-oProxyCommand=bad",
        "user@@host",
        "host:22",
        "host/name",
        "host name",
    ] {
        let mut p = ssh.clone();
        p["ssh_destination"] = json!(bad);
        assert!(Profile::from_value(p).is_err());
    }
    r.profiles = vec![Profile::from_value(ssh).unwrap()];
    r.default_connection_id = r.profiles[0].id().clone();
    r.version = 1;
    assert!(r.validate().is_err());
    r.version = 2;
    assert!(r.validate().is_ok());
}
#[test]
fn atomic_private_round_trip_and_failed_save_preserves_original() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::new(dir.path().join("private/connections.json")).unwrap();
    let r = registry();
    store.save(&r).unwrap();
    assert_eq!(
        store
            .load()
            .unwrap()
            .unwrap()
            .default_connection_id
            .as_str(),
        "alpha"
    );
    let mut bad = r.clone();
    bad.version = 3;
    assert!(store.save(&bad).is_err());
    assert_eq!(store.load().unwrap().unwrap().version, 2);
    store.clear().unwrap();
    assert!(store.load().unwrap().is_none());
}
#[cfg(unix)]
#[test]
fn refuses_symlinks_including_dangling_paths_and_public_permissions() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target");
    std::fs::create_dir(&target).unwrap();
    symlink(&target, dir.path().join("link")).unwrap();
    assert!(
        Store::new(dir.path().join("link/connections.json"))
            .unwrap()
            .save(&registry())
            .is_err()
    );
    symlink(dir.path().join("missing"), target.join("connections.json")).unwrap();
    assert!(
        Store::new(target.join("connections.json"))
            .unwrap()
            .load()
            .is_err()
    );
    std::fs::remove_file(target.join("connections.json")).unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        Store::new(target.join("connections.json"))
            .unwrap()
            .save(&registry())
            .is_err()
    );
}
#[cfg(unix)]
#[test]
fn private_modes_and_atomic_rename_survive_target_symlink_injection() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("private/connections.json");
    let mut store = Store::new(path.clone()).unwrap();
    store.save(&registry()).unwrap();
    assert_eq!(
        std::fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let outside = dir.path().join("outside");
    std::fs::write(&outside, "untouched").unwrap();
    std::fs::remove_file(&path).unwrap();
    symlink(&outside, &path).unwrap();
    assert!(store.save(&registry()).is_err());
    assert_eq!(std::fs::read_to_string(outside).unwrap(), "untouched");
}
