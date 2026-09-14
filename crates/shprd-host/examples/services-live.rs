use axum::Router;
use shprd_host::services::{GenerationIdentity, ServiceState, dispatch_rpc, service_router};
use shprd_workspace::Checkout;
use std::net::SocketAddr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join(format!("shprd-services-live-{}", std::process::id()));
    std::fs::create_dir_all(&root)?;
    std::fs::write(root.join("a+b.txt"), "literal-plus")?;
    std::fs::write(root.join("a b.txt"), "space")?;
    std::fs::write(root.join("preview.pdf"), "%PDF-1.7")?;
    println!("LIVE_ROOT={}", root.display());
    let settings = root.join("settings.json");
    let state = ServiceState::local(
        GenerationIdentity {
            connection_id: "live-local".into(),
            connection_generation: 11,
        },
        Checkout {
            workspace_id: "live-workspace".into(),
            path: root.to_string_lossy().into_owned(),
            repo_name: "live-repo".into(),
        },
        settings,
        || true,
    )?;
    let dispatch = dispatch_rpc(
        &state,
        &serde_json::json!({"id":"live-dispatch","method":"file.list","params":{}}),
    )
    .await?;
    println!("LIVE_DISPATCH={}", serde_json::to_string(&dispatch)?);
    let app = Router::new().merge(service_router(state));
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await?;
    let address = listener.local_addr()?;
    println!("LIVE_ADDR={address}");
    axum::serve(listener, app).await?;
    Ok(())
}
