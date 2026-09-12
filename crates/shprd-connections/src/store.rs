use crate::{Registry, Result, error::invalid, profile::MAX_FILE_BYTES};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

/// Serialized by exclusive mutable access. Explicit paths never silently relax permissions.
#[derive(Debug)]
pub struct Store {
    path: PathBuf,
    harden: bool,
}
impl Store {
    pub fn new(path: PathBuf) -> Result<Self> {
        if !path.is_absolute() || path.file_name().is_none() {
            return Err(invalid("connection registry path must be absolute"));
        }
        Ok(Self {
            path,
            harden: false,
        })
    }
    /// Only the canonical default path may opt into hardening existing permissions.
    pub fn default_path(home: &Path) -> Result<Self> {
        match std::env::var_os("HERDR_GUI_CONNECTIONS_PATH") {
            Some(path) => Self::new(path.into()),
            None => {
                let mut store = Self::new(home.join(".config/herdr-gui/connections.json"))?;
                store.harden = true;
                Ok(store)
            }
        }
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    fn parent(&self) -> Result<&Path> {
        self.path
            .parent()
            .ok_or_else(|| invalid("registry has no parent"))
    }
    fn check_chain(&self) -> Result<()> {
        for path in self.path.ancestors() {
            match fs::symlink_metadata(path) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    return Err(invalid("connection registry path contains a symlink"));
                }
                Ok(_) => (),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }
    fn parent_file(&self, create: bool) -> Result<Option<File>> {
        self.check_chain()?;
        let parent = self.parent()?;
        let exists = parent.try_exists()?;
        if !exists {
            if !create {
                return Ok(None);
            }
            let mut builder = fs::DirBuilder::new();
            builder.recursive(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder.create(parent)?;
        }
        self.check_chain()?;
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(
                (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::DIRECTORY)
                    .bits()
                    .try_into()
                    .map_err(|_| invalid("invalid open flags"))?,
            );
        }
        let directory = options.open(parent)?;
        if !directory.metadata()?.is_dir() {
            return Err(invalid("connection registry parent is not a directory"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if directory.metadata()?.permissions().mode() & 0o077 != 0 {
                if exists && !self.harden {
                    return Err(invalid(
                        "connection registry parent permissions must be 0700",
                    ));
                }
                directory.set_permissions(fs::Permissions::from_mode(0o700))?;
            }
        }
        Ok(Some(directory))
    }
    pub fn load(&self) -> Result<Option<Registry>> {
        let Some(parent) = self.parent_file(false)? else {
            return Ok(None);
        };
        #[cfg(unix)]
        let opened: std::io::Result<File> = rustix::fs::openat(
            &parent,
            self.path
                .file_name()
                .ok_or_else(|| invalid("missing registry name"))?,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::NONBLOCK,
            rustix::fs::Mode::empty(),
        )
        .map(File::from)
        .map_err(Into::into);
        #[cfg(not(unix))]
        let opened = File::open(&self.path);
        let file = match opened {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let meta = file.metadata()?;
        if !meta.is_file() {
            return Err(invalid("connection registry must be a regular file"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if meta.permissions().mode() & 0o077 != 0 {
                if !self.harden {
                    return Err(invalid("connection registry permissions must be 0700/0600"));
                }
                file.set_permissions(fs::Permissions::from_mode(0o600))?;
            }
        }
        if meta.len() > MAX_FILE_BYTES {
            return Err(invalid("connection registry is too large"));
        }
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
        Registry::parse(&bytes).map(Some)
    }
    pub fn save(&mut self, registry: &Registry) -> Result<()> {
        registry.validate()?;
        let mut payload = serde_json::to_vec_pretty(registry)?;
        payload.push(b'\n');
        if payload.len() > 1024 * 1024 {
            return Err(invalid("connection registry is too large"));
        }
        let parent = self
            .parent_file(true)?
            .ok_or_else(|| invalid("missing registry directory"))?;
        let mut temporary = tempfile::Builder::new()
            .prefix(".connections.json.")
            .suffix(".tmp")
            .tempfile_in(self.parent()?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            temporary
                .as_file()
                .set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        temporary.write_all(&payload)?;
        temporary.as_file().sync_all()?;
        self.check_chain()?;
        temporary.persist(&self.path).map_err(|e| e.error)?;
        sync_directory(&parent)?;
        Ok(())
    }
    pub fn clear(&mut self) -> Result<()> {
        let Some(parent) = self.parent_file(false)? else {
            return Ok(());
        };
        self.check_chain()?;
        match fs::remove_file(&self.path) {
            Ok(()) => sync_directory(&parent)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
        Ok(())
    }
}
fn sync_directory(directory: &File) -> Result<()> {
    match directory.sync_all() {
        Ok(()) => Ok(()),
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::InvalidInput | std::io::ErrorKind::Unsupported
            ) =>
        {
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}
