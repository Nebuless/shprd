//! Standalone native SHPRD host entry point.

use clap::Parser;
use shprd_connections::{ConnectionId, LEGACY_ID, Manager, Profile, ProfileService, Store};
use shprd_host::{
    auth::{Auth, load_or_create_token},
    config::Args,
    host,
};
use std::{path::PathBuf, sync::Arc};

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
    let render = args.render_socket(&home);
    let explicit_legacy =
        args.socket_path.is_some() || args.client_socket_path.is_some() || args.session.is_some();
    let manager = Arc::new(Manager::new(ConnectionId::parse(LEGACY_ID)?));
    let profiles = ProfileService::load(
        Store::default_path(&home)?,
        Profile::legacy(
            control.to_str().ok_or("invalid control socket path")?,
            render.to_str().ok_or("invalid render socket path")?,
        )?,
        explicit_legacy,
        Arc::clone(&manager),
        shprd_host::connections::factory(),
    )?;
    let startup_ids = manager
        .list()?
        .into_iter()
        .filter_map(|status| {
            profiles
                .profile(&status.id)
                .ok()
                .filter(|profile| profile.auto_connect() || status.is_default)
                .map(|_| status.id)
        })
        .collect::<Vec<_>>();
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
    let app = host::configured_router_with_profiles(
        control,
        args.public_dir,
        Auth::new(required, secret)?,
        shprd_agent::default_directory().ok(),
        profiles,
        Arc::clone(&manager),
    );
    eprintln!("SHPRD_LISTENING=http://{address}");
    let mut startup = tokio::task::JoinSet::new();
    for id in startup_ids {
        let manager = Arc::clone(&manager);
        startup.spawn(async move { manager.connect(&id).await });
    }
    let served = axum::serve(listener, app)
        .with_graceful_shutdown(async {
            if let Err(error) = tokio::signal::ctrl_c().await {
                eprintln!("shutdown signal: {error}");
            }
        })
        .await;
    let stopped = manager.stop_all().await;
    startup.shutdown().await;
    served?;
    stopped?;
    Ok(())
}
