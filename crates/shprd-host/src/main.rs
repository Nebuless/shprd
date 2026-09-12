//! Standalone native SHPRD host entry point.

use clap::Parser;
use shprd_host::{
    auth::{Auth, load_or_create_token},
    config::Args,
    host,
};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let home = PathBuf::from(
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .ok_or("home directory unavailable")?,
    );
    let listener = tokio::net::TcpListener::bind((args.host.as_str(), args.port)).await?;
    let address = listener.local_addr()?;
    let required = !address.ip().is_loopback();
    let control = args.control_socket(&home);
    let secret = match args.password.filter(|value| !value.is_empty()) {
        Some(secret) => secret,
        None if required => {
            let path = shprd_host::config::config_dir(&home).join("herdr-gui/auth-token");
            let token = load_or_create_token(&path)?;
            eprintln!("Authentication token file: {}", path.display());
            token
        }
        None => String::new(),
    };
    let app = host::configured_router(control, args.public_dir, Auth::new(required, secret)?);
    eprintln!("SHPRD_LISTENING=http://{address}");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            if let Err(error) = tokio::signal::ctrl_c().await {
                eprintln!("shutdown signal: {error}");
            }
        })
        .await?;
    Ok(())
}
