use crate::{Result, error::invalid};
pub(crate) fn local_path(value: &str) -> Result<String> {
    if value.is_empty()
        || value.encode_utf16().count() > 4096
        || value.chars().any(char::is_control)
        || value.split(['/', '\\']).any(|s| s == "..")
    {
        return Err(invalid("invalid absolute socket path"));
    }
    #[cfg(not(windows))]
    {
        if !value.starts_with('/') {
            return Err(invalid("socket path must be absolute"));
        }
        Ok(value.to_owned())
    }
    #[cfg(windows)]
    {
        let value = value.replace('/', "\\");
        if value.to_lowercase().starts_with(r"\\.\pipe\")
            || value.to_lowercase().starts_with(r"\\?\pipe\")
        {
            return Ok(value);
        }
        let b = value.as_bytes();
        if b.len() < 3 || !b[0].is_ascii_alphabetic() || b[1] != b':' || b[2] != b'\\' {
            return Err(invalid("socket path must be absolute"));
        }
        let normalized = value
            .split('\\')
            .filter(|s| !s.is_empty() && *s != ".")
            .collect::<Vec<_>>()
            .join("\\");
        let native = format!(r"\\.\pipe\{normalized}");
        if native.encode_utf16().count() > 4096 {
            return Err(invalid("socket path too long"));
        }
        Ok(native)
    }
}
pub fn validate_destination(value: &str) -> Result<()> {
    let parts: Vec<_> = value.split('@').collect();
    let valid_part = |s: &str, max: usize, user: bool| {
        !s.is_empty()
            && s.len() <= max
            && s.bytes()
                .next()
                .is_some_and(|b| b.is_ascii_alphanumeric() || (user && b == b'_'))
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    };
    let valid = match parts.as_slice() {
        [host] => valid_part(host, 253, false),
        [user, host] => valid_part(user, 64, true) && valid_part(host, 253, false),
        _ => false,
    };
    if !valid || value.len() > 320 {
        return Err(invalid(
            "ssh_destination must be an OpenSSH alias or user@host",
        ));
    }
    Ok(())
}
pub fn validate_remote_path(value: &str) -> Result<()> {
    if value.is_empty() {
        return Ok(());
    }
    if !value.starts_with('/')
        || value.len() > 100
        || value.split('/').skip(1).any(|s| {
            s.is_empty()
                || s == "."
                || s == ".."
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._~+@%=-".contains(&b))
        })
    {
        return Err(invalid(
            "remote socket path must be a short absolute POSIX socket path",
        ));
    }
    Ok(())
}
