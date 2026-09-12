use serde_json::json;
use shprd_connections::{ConnectionId, Manager, parse_http_route, resolve_rpc};
#[test]
fn scoped_http_routes_preserve_id_and_reject_invalid_encoding_or_method() {
    let route = parse_http_route("/api/connections/dev%3Aone/file/download", "GET")
        .unwrap()
        .unwrap();
    assert_eq!(route.connection_id.unwrap().as_str(), "dev:one");
    assert!(parse_http_route("/api/connections/%2E%2E/file/download", "GET").is_err());
    assert!(parse_http_route("/api/connections/dev/file/upload", "GET").is_err());
    assert!(parse_http_route("/api/connections/dev/unknown", "GET").is_err());
    assert!(
        parse_http_route("/api/file/download", "GET")
            .unwrap()
            .unwrap()
            .connection_id
            .is_none()
    );
    assert!(parse_http_route("/health", "GET").unwrap().is_none());
}
#[test]
fn global_rpc_rejects_even_null_identity_and_generations_are_safe_integers() {
    let manager = Manager::new(ConnectionId::parse("default").unwrap());
    assert!(resolve_rpc(&manager, &json!({"id":"1","method":"bridge.ping"})).is_ok());
    assert!(
        resolve_rpc(
            &manager,
            &json!({"id":"1","method":"bridge.ping","connection_id":null})
        )
        .is_err()
    );
    for generation in [
        json!(-1),
        json!(1.5),
        json!(9007199254740992u64),
        json!("1"),
        json!(null),
    ] {
        assert!(
            resolve_rpc(
                &manager,
                &json!({"id":"1","method":"pane.list","connection_generation":generation})
            )
            .is_err()
        );
    }
}
