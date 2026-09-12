use shprd_connections::{
    ConnectionId, Manager, Profile, Runtime, RuntimeContext, RuntimeFuture, SocketPaths,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::Notify;
struct Fixture {
    started: Arc<Notify>,
    release: Arc<Notify>,
    stops: Arc<AtomicUsize>,
    fail: bool,
}
impl Runtime for Fixture {
    fn start<'a>(&'a self, _: &'a RuntimeContext) -> RuntimeFuture<'a, SocketPaths> {
        Box::pin(async move {
            self.started.notify_one();
            self.release.notified().await;
            if self.fail {
                return Err(shprd_connections::Error::Runtime {
                    message: "password=secret startup failed".into(),
                    retryable: false,
                });
            }
            Ok(SocketPaths {
                control: "/tmp/control".into(),
                render: "/tmp/render".into(),
            })
        })
    }
    fn stop(&self) -> RuntimeFuture<'_, ()> {
        Box::pin(async move {
            self.stops.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
}
fn profile() -> Profile {
    Profile::parse(r#"{"id":"alpha","label":"Alpha","type":"local","control_socket_path":"/tmp/control","client_socket_path":"/tmp/render","auto_connect":true}"#).unwrap()
}
#[tokio::test]
async fn disconnect_cancels_pending_start_before_cleanup_and_never_resurrects() {
    let p = profile();
    let id = p.id().clone();
    let manager = Arc::new(Manager::new(id.clone()));
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let stops = Arc::new(AtomicUsize::new(0));
    let fixture = Arc::new(Fixture {
        started: started.clone(),
        release,
        stops: stops.clone(),
        fail: false,
    });
    manager
        .register(p, Arc::new(move |_| Ok(fixture.clone())))
        .unwrap();
    let m = manager.clone();
    let i = id.clone();
    let task = tokio::spawn(async move { m.connect(&i).await });
    tokio::time::timeout(std::time::Duration::from_secs(2), started.notified())
        .await
        .unwrap();
    manager.disconnect(&id).await.unwrap();
    assert!(task.await.unwrap().is_err());
    assert_eq!(stops.load(Ordering::SeqCst), 1);
    assert_eq!(
        manager.status(&id).unwrap().state,
        shprd_connections::State::Disconnected
    );
    assert!(manager.lease(&id).is_err());
}
#[tokio::test]
async fn old_lease_is_invalid_after_disconnect_and_reregister() {
    let p = profile();
    let id = p.id().clone();
    let manager = Manager::new(id.clone());
    let release = Arc::new(Notify::new());
    release.notify_one();
    let fixture = Arc::new(Fixture {
        started: Arc::new(Notify::new()),
        release,
        stops: Arc::new(AtomicUsize::new(0)),
        fail: false,
    });
    manager
        .register(p, Arc::new(move |_| Ok(fixture.clone())))
        .unwrap();
    manager.connect(&id).await.unwrap();
    let lease = manager.lease(&id).unwrap();
    assert!(lease.is_current());
    manager.disconnect(&id).await.unwrap();
    assert!(!lease.is_current());
    assert!(
        manager
            .resolve(Some(&id), Some(lease.generation()))
            .is_err()
    );
    assert!(ConnectionId::parse("bad/id").is_err());
}
#[tokio::test]
async fn failed_start_stops_runtime_and_redacts_status() {
    let p = profile();
    let id = p.id().clone();
    let manager = Manager::new(id.clone());
    let release = Arc::new(Notify::new());
    release.notify_one();
    let stops = Arc::new(AtomicUsize::new(0));
    let fixture = Arc::new(Fixture {
        started: Arc::new(Notify::new()),
        release,
        stops: stops.clone(),
        fail: true,
    });
    manager
        .register(p, Arc::new(move |_| Ok(fixture.clone())))
        .unwrap();
    assert!(manager.connect(&id).await.is_err());
    assert_eq!(stops.load(Ordering::SeqCst), 1);
    let status = serde_json::to_string(&manager.status(&id).unwrap()).unwrap();
    assert!(!status.contains("secret"));
}
