use serde_json::json;
use shprd_connections::*;
use std::sync::Arc;

#[tokio::test]
async fn explicit_disconnect_during_ready_update_preflight_does_not_reconnect() {
    let dir = tempfile::tempdir().unwrap();
    let manager = Arc::new(Manager::new(ConnectionId::parse("legacy-default").unwrap()));
    let mut service = ProfileService::load(
        Store::new(dir.path().join("private/connections.json")).unwrap(),
        Profile::legacy("/tmp/c", "/tmp/r").unwrap(),
        false,
        manager.clone(),
        Arc::new(|_| Arc::new(|_| Ok(Arc::new(Endpoint { fail: false })))),
    )
    .unwrap();
    let id = ConnectionId::parse("alpha").unwrap();
    service.create(p("alpha", "Before")).await.unwrap();
    assert_eq!(manager.status(&id).unwrap().state, State::Ready);
    let (entered, probed) = tokio::sync::oneshot::channel();
    let (release, released) = tokio::sync::oneshot::channel();
    // Given a Ready connection, stop it while replacement preflight holds update open.
    let update = service.update(&id, p("alpha", "After"), |_| async {
        entered.send(()).unwrap();
        released.await.unwrap();
        Ok(ProbeResult {
            ok: true,
            version: None,
            protocol: 22,
        })
    });
    let stop = async {
        probed.await.unwrap();
        manager.disconnect(&id).await.unwrap();
        release.send(()).unwrap();
    };
    let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        tokio::join!(update, stop)
    })
    .await
    .unwrap();
    assert!(result.is_err());
    assert_eq!(manager.status(&id).unwrap().state, State::Disconnected);
    assert_eq!(service.profile(&id).unwrap().label(), "Before");
}

#[tokio::test]
async fn explicit_disconnect_during_ready_update_rollback_does_not_reconnect() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Notify;
    struct Counted {
        starts: Arc<AtomicUsize>,
        entered: Arc<Notify>,
        hold: bool,
    }
    impl Runtime for Counted {
        fn start<'a>(&'a self, _: &'a RuntimeContext) -> RuntimeFuture<'a, SocketPaths> {
            Box::pin(async move {
                self.starts.fetch_add(1, Ordering::SeqCst);
                if self.hold {
                    self.entered.notify_one();
                    std::future::pending::<()>().await;
                }
                Ok(SocketPaths {
                    control: "/tmp/c".into(),
                    render: "/tmp/r".into(),
                })
            })
        }
        fn stop(&self) -> RuntimeFuture<'_, ()> {
            Box::pin(async { Ok(()) })
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("private/connections.json");
    let manager = Arc::new(Manager::new(ConnectionId::parse("legacy-default").unwrap()));
    let starts = Arc::new(AtomicUsize::new(0));
    let entered = Arc::new(Notify::new());
    let count = starts.clone();
    let signal = entered.clone();
    let mut service = ProfileService::load(
        Store::new(path.clone()).unwrap(),
        Profile::legacy("/tmp/c", "/tmp/r").unwrap(),
        true,
        manager.clone(),
        Arc::new(move |p| {
            let hold = p.label() == "After";
            let count = count.clone();
            let signal = signal.clone();
            Arc::new(move |_| {
                Ok(Arc::new(Counted {
                    starts: count.clone(),
                    entered: signal.clone(),
                    hold,
                }))
            })
        }),
    )
    .unwrap();
    let id = ConnectionId::parse("alpha").unwrap();
    service.create(p("alpha", "Before")).await.unwrap();
    manager.connect(&id).await.unwrap();
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    // Given a Ready connection, stop it after update persists and starts replacement.
    let update = service.update(&id, p("alpha", "After"), |_| async {
        Ok(ProbeResult {
            ok: true,
            version: None,
            protocol: 22,
        })
    });
    let stop = async {
        entered.notified().await;
        manager.disconnect(&id).await.unwrap();
    };
    let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        tokio::join!(update, stop)
    })
    .await
    .unwrap();
    assert!(result.is_err());
    assert_eq!(manager.status(&id).unwrap().state, State::Disconnected);
    assert_eq!(
        starts.load(Ordering::SeqCst),
        2,
        "rollback must not start original runtime again"
    );
    assert_eq!(
        Store::new(path).unwrap().load().unwrap().unwrap().profiles[0].label(),
        "Before"
    );
}

struct Endpoint {
    fail: bool,
}
impl Runtime for Endpoint {
    fn start<'a>(&'a self, _: &'a RuntimeContext) -> RuntimeFuture<'a, SocketPaths> {
        Box::pin(async {
            if self.fail {
                return Err(Error::Runtime {
                    message: "fixture failed".into(),
                    retryable: false,
                });
            }
            Ok(SocketPaths {
                control: "/tmp/c".into(),
                render: "/tmp/r".into(),
            })
        })
    }
    fn stop(&self) -> RuntimeFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
}
fn p(id: &str, label: &str) -> Profile {
    Profile::from_value(json!({"id":id,"label":label,"type":"local","control_socket_path":"/tmp/c","client_socket_path":"/tmp/r","auto_connect":false})).unwrap()
}
#[tokio::test]
async fn failed_update_restores_disk_profile_runtime_and_invalidates_old_reply() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("private/connections.json");
    let manager = Arc::new(Manager::new(ConnectionId::parse("legacy-default").unwrap()));
    let mut service = ProfileService::load(
        Store::new(path.clone()).unwrap(),
        Profile::legacy("/tmp/c", "/tmp/r").unwrap(),
        false,
        manager.clone(),
        Arc::new(|p| {
            let fail = p.label() == "Fail";
            Arc::new(move |_| Ok(Arc::new(Endpoint { fail })))
        }),
    )
    .unwrap();
    let id = ConnectionId::parse("alpha").unwrap();
    service.create(p("alpha", "Before")).await.unwrap();
    let lease = manager.lease(&id).unwrap();
    let mut publisher = ReplyPublisher::new(lease.clone(), "request".into());
    let result = service
        .update(&id, p("alpha", "Fail"), |_| async {
            Ok(ProbeResult {
                ok: true,
                version: Some("fixture".into()),
                protocol: 22,
            })
        })
        .await;
    assert!(result.is_err());
    assert_eq!(service.item(&id).unwrap()["label"], "Before");
    assert_eq!(
        Store::new(path)
            .unwrap()
            .load()
            .unwrap()
            .unwrap()
            .profiles
            .iter()
            .find(|p| p.id() == &id)
            .unwrap()
            .label(),
        "Before"
    );
    assert!(manager.lease(&id).is_ok());
    assert!(!lease.is_current());
    assert!(lease.checked_chunk(b"stale").is_err());
    let reply = publisher
        .publish(
            json!({"id":"request","result":{"ok":true}})
                .as_object()
                .unwrap()
                .clone(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        reply["error"]["message"],
        "connection changed during request"
    );
    assert_eq!(reply["connection_generation"], lease.generation());
    assert!(publisher.publish(Default::default()).unwrap().is_none());
}
#[tokio::test]
async fn failed_persistence_does_not_add_profile_or_replace_registry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("private/connections.json");
    let manager = Arc::new(Manager::new(ConnectionId::parse("legacy-default").unwrap()));
    let mut service = ProfileService::load(
        Store::new(path.clone()).unwrap(),
        Profile::legacy("/tmp/c", "/tmp/r").unwrap(),
        false,
        manager,
        Arc::new(|_| Arc::new(|_| Ok(Arc::new(Endpoint { fail: false })))),
    )
    .unwrap();
    service.create(p("alpha", "Alpha")).await.unwrap();
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(service.create(p("beta", "Beta")).await.is_err());
    assert!(
        service
            .profile(&ConnectionId::parse("beta").unwrap())
            .is_err()
    );
    assert_eq!(service.list().unwrap().len(), 2);
}
#[test]
fn supported_protocol_matrix_rejects_twenty_one() {
    for protocol in 0..=24 {
        assert_eq!(
            ProbeResult {
                ok: true,
                version: None,
                protocol
            }
            .validate()
            .is_ok(),
            matches!(protocol, 14..=20 | 22)
        );
    }
}
