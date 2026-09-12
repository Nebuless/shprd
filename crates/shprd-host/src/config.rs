//! Command-line configuration. Explicit arguments take precedence over environment.

use clap::Parser;
use std::path::{Path, PathBuf};

#[derive(Debug, Parser)]
#[command(name = "shprd", version, about = "SHPRD native host")]
pub struct Args {
    #[arg(long, env = "HOST", default_value = "127.0.0.1")]
    pub host: String,
    #[arg(long, env = "PORT", default_value_t = 8787)]
    pub port: u16,
    #[arg(long, env = "HERDR_GUI_PASSWORD", hide_env_values = true)]
    pub password: Option<String>,
    #[arg(long, env = "HERDR_SOCKET_PATH")]
    pub socket_path: Option<PathBuf>,
    #[arg(long, env = "HERDR_CLIENT_SOCKET_PATH")]
    pub client_socket_path: Option<PathBuf>,
    #[arg(long, env = "HERDR_SESSION", value_parser = session_name)]
    pub session: Option<String>,
    #[arg(long, env = "PUBLIC_DIR", default_value = "web/dist")]
    pub public_dir: PathBuf,
    #[arg(long, env = "OPEN_BROWSER", action = clap::ArgAction::SetTrue)]
    pub open: bool,
}

fn session_name(value: &str) -> Result<String, String> {
    if value.is_empty() || value == "." || value == ".." || value.contains(['/', '\\']) {
        return Err("session must be a single directory name".to_owned());
    }
    Ok(value.to_owned())
}

impl Args {
    pub fn control_socket(&self, home: &Path) -> PathBuf {
        let path = self
            .socket_path
            .clone()
            .unwrap_or_else(|| self.session_dir(home).join("herdr.sock"));
        PathBuf::from(native_socket_path(&path.to_string_lossy(), cfg!(windows)))
    }

    pub fn render_socket(&self, home: &Path) -> PathBuf {
        let path = self
            .client_socket_path
            .clone()
            .unwrap_or_else(|| self.session_dir(home).join("herdr-client.sock"));
        PathBuf::from(native_socket_path(&path.to_string_lossy(), cfg!(windows)))
    }

    fn session_dir(&self, home: &Path) -> PathBuf {
        let base = config_dir(home).join("herdr");
        match &self.session {
            Some(session) => base.join("sessions").join(session),
            None => base,
        }
    }
}

pub fn config_dir(home: &Path) -> PathBuf {
    if cfg!(windows) {
        std::env::var_os("APPDATA").map_or_else(|| home.join("AppData/Roaming"), PathBuf::from)
    } else {
        home.join(".config")
    }
}

pub fn native_socket_path(path: &str, windows: bool) -> String {
    if windows
        && !path.to_ascii_lowercase().starts_with(r"\\.\pipe\")
        && !path.to_ascii_lowercase().starts_with(r"\\?\pipe\")
    {
        format!(r"\\.\pipe\{path}")
    } else {
        path.to_owned()
    }
}
