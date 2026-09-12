use serde_json::json;
use shprd_connections::*;
use std::sync::Arc;
use tokio::sync::Notify;
struct BlockStop {
    entered: Arc<Notify>,
    release: Arc<Notify>,
}
impl Runtime for BlockStop {
    fn start<'a>(&'a self, _: &'a RuntimeContext) -> RuntimeFuture<'a, SocketPaths> {
        Box::pin(async {
            Ok(SocketPaths {
                control: "/tmp/c".into(),
                render: "/tmp/r".into(),
            })
        })
    }
    fn stop(&self) -> RuntimeFuture<'_, ()> {
        Box::pin(async {
            self.entered.notify_one();
            self.release.notified().await;
            Ok(())
        })
    }
}
fn local(id: &str) -> Profile {
    Profile::from_value(json!({"id":id,"label":id,"type":"local","control_socket_path":"/tmp/c","client_socket_path":"/tmp/r","auto_connect":false})).unwrap()
}
#[tokio::test]
async fn shutdown_invalidates_all_leases_before_first_slow_cleanup() {
    let manager = Arc::new(Manager::new(ConnectionId::parse("a").unwrap()));
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    for id in ["a", "b"] {
        let runtime = Arc::new(BlockStop {
            entered: entered.clone(),
            release: release.clone(),
        });
        manager
            .register(local(id), Arc::new(move |_| Ok(runtime.clone())))
            .unwrap();
        manager
            .connect(&ConnectionId::parse(id).unwrap())
            .await
            .unwrap();
    }
    let peer = manager.lease(&ConnectionId::parse("b").unwrap()).unwrap();
    let m = manager.clone();
    let task = tokio::spawn(async move { m.stop_all().await });
    tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified())
        .await
        .unwrap();
    let current_during_cleanup = peer.is_current();
    release.notify_one();
    tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified())
        .await
        .unwrap();
    release.notify_one();
    task.await.unwrap().unwrap();
    assert!(!current_during_cleanup);
}
#[test]
fn redaction_preserves_url_scheme_without_credentials() {
    assert_eq!(
        sanitize_error("https://user:password@example.test/path"),
        "https://***@example.test/path"
    );
    assert!(
        !sanitize_error("Authorization: Bearer abc password=private token='hidden'")
            .contains("private")
    );
    assert!(
        !sanitize_error("Authorization: Bearer abc password=private token='hidden'")
            .contains("hidden")
    );
}
#[test]
fn event_envelopes_reject_reserved_fields_and_generation_queries() {
    let id = ConnectionId::parse("alpha").unwrap();
    for field in [
        "connection_id",
        "connection_generation",
        "hello",
        "id",
        "result",
        "error",
        "control",
        "terminal",
        "terminal_clipboard",
    ] {
        let mut event = json!({"event":"pane.created","data":{}});
        event[field] = json!(null);
        assert!(event_envelope(&id, Some(1), event).is_err());
    }
    assert_eq!(
        event_envelope(&id, Some(2), json!({"event":"pane.created","data":{}})).unwrap()["connection_generation"],
        2
    );
    for query in ["", "1.0", "+1", "-1", "1e2", "9007199254740992"] {
        assert!(query_generation(Some(query)).is_err());
    }
    assert_eq!(query_generation(Some("00012")).unwrap(), Some(12));
}
