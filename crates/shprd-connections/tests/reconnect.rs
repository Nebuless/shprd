use shprd_connections::*;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::Notify;
struct Retiring {
    context: Arc<Mutex<Option<RuntimeContext>>>,
    entered: Arc<Notify>,
    release: Arc<Notify>,
    starts: Arc<AtomicUsize>,
}
impl Runtime for Retiring {
    fn start<'a>(&'a self, context: &'a RuntimeContext) -> RuntimeFuture<'a, SocketPaths> {
        Box::pin(async move {
            self.starts.fetch_add(1, Ordering::SeqCst);
            *self.context.lock().unwrap() = Some(context.clone());
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
#[tokio::test]
async fn disconnect_during_failed_runtime_retirement_prevents_replacement_start() {
    let profile = Profile::parse(r#"{"id":"a","label":"A","type":"local","control_socket_path":"/tmp/c","client_socket_path":"/tmp/r","auto_connect":false}"#).unwrap();
    let id = profile.id().clone();
    let manager = Arc::new(Manager::new(id.clone()));
    let context = Arc::new(Mutex::new(None));
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let starts = Arc::new(AtomicUsize::new(0));
    let fixture = Arc::new(Retiring {
        context: context.clone(),
        entered: entered.clone(),
        release: release.clone(),
        starts: starts.clone(),
    });
    manager
        .register(profile, Arc::new(move |_| Ok(fixture.clone())))
        .unwrap();
    manager.connect(&id).await.unwrap();
    context
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .report_error("lost transport", true)
        .unwrap();
    let m = manager.clone();
    let i = id.clone();
    let retry = tokio::spawn(async move { m.connect(&i).await });
    tokio::time::timeout(std::time::Duration::from_secs(2), entered.notified())
        .await
        .unwrap();
    let mut stop = Box::pin(manager.disconnect(&id));
    std::future::poll_fn(|cx| {
        assert!(stop.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    release.notify_one();
    let outcome = retry.await.unwrap();
    release.notify_one();
    stop.await.unwrap();
    assert!(outcome.is_err());
    assert_eq!(starts.load(Ordering::SeqCst), 1);
}
