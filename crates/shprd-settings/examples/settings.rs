//! Read one explicit settings file without touching the user's home directory.
use shprd_settings::{SettingsIdentity, SettingsService};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: settings SETTINGS_PATH [CONNECTION_ID] [SSH_HOST]")?;
    let service = SettingsService::new(
        path,
        SettingsIdentity {
            connection_id: args.next(),
            host: args.next(),
        },
    )?;
    println!("{}", serde_json::to_string_pretty(&service.read().await?)?);
    Ok(())
}
