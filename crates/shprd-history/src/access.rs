use crate::{FileMeta, Result, error::invalid};
use std::{fs, io::Read, path::Path};

pub struct LocalFiles;

impl LocalFiles {
    pub fn metadata(path: &Path) -> Result<Option<FileMeta>> {
        let info = match fs::metadata(path) {
            Ok(info) => info,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if !info.is_file() {
            return Ok(None);
        }
        let mtime_ms = info
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|duration| u64::try_from(duration.as_millis()).ok())
            .unwrap_or(0);
        Ok(Some(FileMeta {
            path: path.to_owned(),
            mtime_ms,
            size: Some(info.len()),
            identity: None,
            change_token: None,
            session_id: None,
            created_at_ms: None,
            model_name: None,
            agent_version: None,
        }))
    }

    pub fn read_text(path: &Path) -> Result<String> {
        if !path.is_absolute() {
            return Err(invalid("session transcript path must be absolute"));
        }
        Ok(fs::read_to_string(path)?)
    }

    pub fn read_prefix(path: &Path, limit: usize) -> Result<Vec<u8>> {
        if !path.is_absolute() {
            return Err(invalid("session transcript path must be absolute"));
        }
        let mut bytes = Vec::with_capacity(limit.min(64 * 1024));
        fs::File::open(path)?
            .take(limit as u64)
            .read_to_end(&mut bytes)?;
        Ok(bytes)
    }
}
