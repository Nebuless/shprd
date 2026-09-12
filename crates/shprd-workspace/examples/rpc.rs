//! One-request smoke entry point; the production host supplies the resolved checkout.
use serde_json::Value;
use shprd_workspace::{Checkout, HostConfig, WorkspaceService};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: rpc CHECKOUT METHOD PARAMS_JSON")?;
    let method = args.next().ok_or("missing method")?;
    let params: Value = serde_json::from_str(&args.next().ok_or("missing params JSON")?)?;
    let service = WorkspaceService::new(HostConfig::Local)?;
    let result = service
        .dispatch(
            &Checkout {
                workspace_id: "smoke".into(),
                repo_name: "smoke".into(),
                path,
            },
            &method,
            &params,
        )
        .await?;
    println!("{result}");
    Ok(())
}
