use shprd_connections::{
    ConnectionId, Manager, Profile, ProfileService, Runtime, RuntimeContext, RuntimeFuture,
    SocketPaths, Store,
};
use std::sync::Arc;
struct Local;
impl Runtime for Local {
    fn start<'a>(&'a self, _: &'a RuntimeContext) -> RuntimeFuture<'a, SocketPaths> {
        Box::pin(async {
            Ok(SocketPaths {
                control: "/tmp/control".into(),
                render: "/tmp/render".into(),
            })
        })
    }
    fn stop(&self) -> RuntimeFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
}
fn profile(id: &str) -> Profile {
    Profile::parse(&format!(r#"{{"id":"{id}","label":"Local","type":"local","control_socket_path":"/tmp/control","client_socket_path":"/tmp/render","auto_connect":false}}"#)).unwrap()
}
#[tokio::test]
async fn first_create_migrates_legacy_and_crud_is_durable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("private/connections.json");
    let manager = Arc::new(Manager::new(ConnectionId::parse("legacy-default").unwrap()));
    let mut service = ProfileService::load(
        Store::new(path.clone()).unwrap(),
        Profile::legacy("/tmp/control", "/tmp/render").unwrap(),
        false,
        manager.clone(),
        Arc::new(|_| Arc::new(|_| Ok(Arc::new(Local)))),
    )
    .unwrap();
    assert!(
        service
            .remove(&ConnectionId::parse("legacy-default").unwrap())
            .await
            .is_err()
    );
    service.create(profile("remote")).await.unwrap();
    assert_eq!(manager.default_id().unwrap().as_str(), "remote");
    assert_eq!(
        Store::new(path.clone())
            .unwrap()
            .load()
            .unwrap()
            .unwrap()
            .profiles
            .len(),
        2
    );
    assert!(
        service
            .list()
            .unwrap()
            .iter()
            .all(|p| p["read_only"] == false)
    );
    service
        .set_default(&ConnectionId::parse("local").unwrap())
        .unwrap();
    service
        .remove(&ConnectionId::parse("remote").unwrap())
        .await
        .unwrap();
    assert_eq!(
        Store::new(path)
            .unwrap()
            .load()
            .unwrap()
            .unwrap()
            .profiles
            .len(),
        1
    );
}
#[tokio::test]
async fn explicit_legacy_default_cannot_be_changed() {
    let dir = tempfile::tempdir().unwrap();
    let manager = Arc::new(Manager::new(ConnectionId::parse("legacy-default").unwrap()));
    let mut service = ProfileService::load(
        Store::new(dir.path().join("private/connections.json")).unwrap(),
        Profile::legacy("/tmp/control", "/tmp/render").unwrap(),
        true,
        manager.clone(),
        Arc::new(|_| Arc::new(|_| Ok(Arc::new(Local)))),
    )
    .unwrap();
    service.create(profile("new")).await.unwrap();
    assert!(
        service
            .set_default(&ConnectionId::parse("new").unwrap())
            .is_err()
    );
    assert_eq!(manager.default_id().unwrap().as_str(), "legacy-default");
}
