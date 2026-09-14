use crate::{
    Checkout, Error, HostConfig, LIST_LIMIT, PREVIEW_IMAGE_MAX_BYTES, PREVIEW_MAX_BYTES, Result,
    path, process, remote, required_path, string,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
    time::{Duration, UNIX_EPOCH},
};
use tokio::{
    fs,
    io::{AsyncRead, AsyncReadExt},
};

#[derive(Debug)]
pub struct Download {
    pub filename: String,
    pub path: String,
    pub size: u64,
    pub headers: BTreeMap<String, String>,
    pub body: fs::File,
    pub(crate) _temporary: Option<tempfile::NamedTempFile>,
}

pub(crate) fn inside(root: &Path, target: &Path) -> Result<()> {
    if !target.starts_with(root) {
        return Err(Error::Invalid(
            "file explorer path escaped the workspace checkout".into(),
        ));
    }
    Ok(())
}
pub(crate) fn display(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
pub(crate) fn mtime(meta: &std::fs::Metadata) -> f64 {
    meta.modified()
        .ok()
        .and_then(|v| v.duration_since(UNIX_EPOCH).ok())
        .map_or(0.0, |v| v.as_secs_f64() * 1000.0)
}
async fn target(root: &str, requested: &str, absolute: bool) -> Result<(PathBuf, PathBuf)> {
    let root = fs::canonicalize(root).await?;
    let target = fs::canonicalize(root.join(requested)).await?;
    if !absolute {
        inside(&root, &target)?;
    }
    Ok((root, target))
}
fn kind(meta: &std::fs::Metadata) -> &'static str {
    if meta.is_symlink() {
        "symlink"
    } else if meta.is_dir() {
        "directory"
    } else {
        "file"
    }
}
pub(crate) fn mime(path: &str) -> Option<&'static str> {
    match path
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "bmp" => Some("image/bmp"),
        "ico" => Some("image/x-icon"),
        "avif" => Some("image/avif"),
        _ => None,
    }
}
pub(crate) fn decode(bytes: &[u8], truncated: bool, path: &str) -> Value {
    let mime = mime(path);
    if let Some(mime) = mime
        && !truncated
    {
        return json!({"text":null,"binary":true,"mime_type":mime,"image_data_url":format!("data:{mime};base64,{}",STANDARD.encode(bytes))});
    }
    if !bytes.contains(&0) {
        let text = match std::str::from_utf8(bytes) {
            Ok(text) => Some(text),
            Err(error) if truncated && error.error_len().is_none() => {
                std::str::from_utf8(&bytes[..error.valid_up_to()]).ok()
            }
            Err(_) => None,
        };
        if let Some(text) = text {
            return json!({"text":text,"binary":false});
        }
    }
    let mut value = json!({"text":null,"binary":true});
    if let Some(mime) = mime {
        value["mime_type"] = json!(mime);
    }
    value
}
pub(crate) async fn dispatch<F: Fn() -> bool>(
    host: &HostConfig,
    checkout: &Checkout,
    method: &str,
    params: &Value,
    current: &F,
) -> Result<Value> {
    if method == "file.resolve" {
        return resolve(host, checkout, params).await;
    }
    if matches!(host, HostConfig::Ssh { .. }) {
        return remote::dispatch(host, checkout, method, params, current).await;
    }
    match method {
        "file.list" => {
            let relative = path(string(params, "path"), false)?;
            let (root, dir) = target(&checkout.path, &relative, false).await?;
            let mut reader = fs::read_dir(&dir).await?;
            let mut entries = Vec::new();
            while let Some(entry) = reader.next_entry().await? {
                let name = entry.file_name().to_string_lossy().into_owned();
                let hidden = name.starts_with('.');
                if hidden && params["show_hidden"] != true {
                    continue;
                }
                let meta = fs::metadata(entry.path()).await.ok();
                let ty = entry.file_type().await?;
                entries.push(json!({"name":name,"path":if relative.is_empty() { name.clone() } else { format!("{relative}/{name}") },"type":if ty.is_symlink(){"symlink"}else if ty.is_dir(){"directory"}else{"file"},"size":meta.as_ref().map_or(0,|m|m.len()),"mtime_ms":meta.as_ref().map_or(0.0,mtime),"hidden":hidden}));
            }
            sort_entries(&mut entries);
            let truncated = entries.len() > LIST_LIMIT;
            entries.truncate(LIST_LIMIT);
            ignored(host, &display(&dir), &mut entries).await;
            Ok(
                json!({"root":display(&root),"path":relative,"entries":entries,"truncated":truncated}),
            )
        }
        "file.read" => {
            let requested = required_path(params, true)?;
            let absolute = requested.starts_with('/');
            let (root, file) = target(&checkout.path, &requested, absolute).await?;
            let meta = fs::metadata(&file).await?;
            if !meta.is_file() {
                return Err(Error::Invalid("only regular files can be previewed".into()));
            }
            let display_path = if absolute {
                display(&file)
            } else {
                display(
                    file.strip_prefix(&root)
                        .map_err(|_| Error::Invalid("path escaped root".into()))?,
                )
            };
            let limit =
                if mime(&display_path).is_some() && meta.len() <= PREVIEW_IMAGE_MAX_BYTES as u64 {
                    PREVIEW_IMAGE_MAX_BYTES
                } else {
                    PREVIEW_MAX_BYTES
                };
            let mut bytes = Vec::new();
            fs::File::open(&file)
                .await?
                .take((limit + 1) as u64)
                .read_to_end(&mut bytes)
                .await?;
            let truncated = meta.len() > limit as u64 || bytes.len() > limit;
            bytes.truncate(limit);
            let mut value = decode(&bytes, truncated, &display_path);
            value["root"] = json!(display(&root));
            value["path"] = json!(display_path);
            value["size"] = json!(meta.len());
            value["mtime_ms"] = json!(mtime(&meta));
            value["truncated"] = json!(truncated);
            Ok(value)
        }
        "file.delete" => {
            if !current() {
                return Err(Error::Stale("connection generation changed".into()));
            }
            let requested = required_path(params, false)?;
            let root = fs::canonicalize(&checkout.path).await?;
            let target = root.join(&requested);
            let parent = fs::canonicalize(
                target
                    .parent()
                    .ok_or_else(|| Error::Invalid("missing parent".into()))?,
            )
            .await?;
            inside(&root, &parent)?;
            let meta = fs::symlink_metadata(&target).await?;
            if !current() {
                return Err(Error::Stale("connection generation changed".into()));
            }
            if meta.is_dir() {
                fs::remove_dir_all(&target).await?;
            } else {
                fs::remove_file(&target).await?;
            }
            Ok(json!({"path":requested,"type":kind(&meta)}))
        }
        _ => Err(Error::Invalid("unknown file method".into())),
    }
}
pub(crate) fn sort_entries(entries: &mut [Value]) {
    entries.sort_by(|a, b| {
        (a["type"] != "directory")
            .cmp(&(b["type"] != "directory"))
            .then_with(|| {
                string(a, "name")
                    .to_lowercase()
                    .cmp(&string(b, "name").to_lowercase())
            })
    });
}
pub(crate) async fn ignored(host: &HostConfig, dir: &str, entries: &mut [Value]) {
    if entries.is_empty() {
        return;
    }
    let mut input = Vec::new();
    for entry in entries.iter() {
        input.extend_from_slice(string(entry, "name").as_bytes());
        input.push(0);
    }
    if let Ok(output) = process::run(
        host,
        &["git", "-C", dir, "check-ignore", "-z", "--stdin"],
        Some(&input),
        10,
        1024 * 1024,
    )
    .await
        && (output.code == 0 || output.code == 1)
    {
        let text = output.text();
        let names: HashSet<_> = text.split('\0').collect();
        for entry in entries {
            if names.contains(string(entry, "name")) {
                entry["ignored"] = json!(true);
            }
        }
    }
}
async fn resolve(host: &HostConfig, checkout: &Checkout, params: &Value) -> Result<Value> {
    let mut seen = HashSet::new();
    let mut candidates = Vec::new();
    if let Some(paths) = params["paths"].as_array() {
        for candidate in paths
            .iter()
            .take(32)
            .filter_map(Value::as_str)
            .map(str::trim)
        {
            if candidate.is_empty() || candidate.chars().count() > 4096 || !seen.insert(candidate) {
                continue;
            }
            let Ok(mut path) = path(candidate, true) else {
                continue;
            };
            if !path.starts_with('/') {
                path = path
                    .split('/')
                    .filter(|p| !p.is_empty() && *p != ".")
                    .collect::<Vec<_>>()
                    .join("/");
            }
            if !path.is_empty() {
                candidates.push((candidate.to_owned(), path));
            }
        }
    }
    if candidates.is_empty() {
        return Ok(json!({"workspace_id":checkout.workspace_id,"files":[]}));
    }
    let mut files = Vec::new();
    for (candidate, requested) in candidates {
        let exists = match host {
            HostConfig::Local => {
                match target(&checkout.path, &requested, requested.starts_with('/')).await {
                    Ok((_, file)) => fs::metadata(file).await.is_ok_and(|m| m.is_file()),
                    Err(_) => false,
                }
            }
            HostConfig::Ssh { .. } => {
                remote::regular_file(host, &checkout.path, &requested).await?
            }
        };
        if exists {
            files.push(json!({"candidate":candidate,"path":requested}));
        }
    }
    Ok(json!({"workspace_id":checkout.workspace_id,"checkout_path":checkout.path,"files":files}))
}
pub(crate) fn filename(params: &Value) -> Result<String> {
    let name = string(params, "filename").trim();
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', '\0']) {
        return Err(Error::Invalid("invalid upload filename".into()));
    }
    Ok(name.to_owned())
}
pub(crate) async fn upload<R: AsyncRead + Unpin, F: Fn() -> bool + Sync>(
    host: &HostConfig,
    checkout: &Checkout,
    params: &Value,
    body: &mut R,
    current: &F,
) -> Result<Value> {
    let directory = path(string(params, "directory"), false)?;
    let name = filename(params)?;
    if matches!(host, HostConfig::Ssh { .. }) {
        return remote::upload(host, checkout, &directory, &name, body, current).await;
    }
    let (root, dir) = target(&checkout.path, &directory, false).await?;
    if !fs::metadata(&dir).await?.is_dir() {
        return Err(Error::Invalid("upload target is not a directory".into()));
    }
    let destination = dir.join(&name);
    let existing = match fs::symlink_metadata(&destination).await {
        Ok(meta) => {
            if !meta.is_file() || meta.is_symlink() {
                return Err(Error::Invalid(
                    "cannot overwrite a directory or symlink".into(),
                ));
            }
            Some(meta)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let temp = tempfile::NamedTempFile::new_in(&dir)?;
    let mut writer = fs::File::from_std(temp.reopen()?);
    let size = tokio::time::timeout(Duration::from_secs(120), async {
        let mut buffer = [0u8; 64 * 1024];
        let mut size = 0u64;
        loop {
            if !current() {
                return Err(Error::Stale("connection generation changed".into()));
            }
            let count = body.read(&mut buffer).await?;
            if count == 0 {
                break;
            }
            tokio::io::AsyncWriteExt::write_all(&mut writer, &buffer[..count]).await?;
            size = size.saturating_add(count as u64);
        }
        Ok::<u64, Error>(size)
    })
    .await
    .map_err(|_| Error::Timeout)??;
    if !current() {
        return Err(Error::Stale("connection generation changed".into()));
    }
    writer.sync_all().await?;
    if let Some(meta) = existing.as_ref() {
        fs::set_permissions(temp.path(), meta.permissions()).await?;
    }
    if !current() {
        return Err(Error::Stale("connection generation changed".into()));
    }
    let destination_state = fs::symlink_metadata(&destination).await;
    if !current() {
        return Err(Error::Stale("connection generation changed".into()));
    }
    match (&existing, destination_state) {
        (Some(old), Ok(now))
            if now.is_file()
                && !now.is_symlink()
                && old.len() == now.len()
                && old.modified().ok() == now.modified().ok() =>
        {
            temp.persist(&destination).map_err(|e| Error::Io(e.error))?;
        }
        (None, Err(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            temp.persist_noclobber(&destination)
                .map_err(|e| Error::Io(e.error))?;
        }
        _ => return Err(Error::Stale("upload target changed during upload".into())),
    }
    Ok(
        json!({"workspace_id":checkout.workspace_id,"directory":directory,"filename":name,"path":display(destination.strip_prefix(root).map_err(|_|Error::Invalid("path escaped root".into()))?),"size":size,"overwritten":existing.is_some()}),
    )
}
pub(crate) fn encode(value: &str) -> String {
    let mut result = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            result.push(char::from(byte));
        } else {
            use std::fmt::Write;
            let _ = write!(result, "%{byte:02X}");
        }
    }
    result
}
pub(crate) fn headers(
    path: &str,
    filename: &str,
    size: u64,
    archive: bool,
    inline: bool,
) -> BTreeMap<String, String> {
    let inline_mime = if inline && !archive {
        if path.to_ascii_lowercase().ends_with(".pdf") {
            Some("application/pdf")
        } else {
            mime(path)
        }
    } else {
        None
    };
    let disposition = if inline_mime.is_some() {
        "inline"
    } else {
        "attachment"
    };
    let fallback: String = filename
        .chars()
        .map(|c| {
            if c.is_ascii() && (' '..='~').contains(&c) && c != '"' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let mut headers = BTreeMap::from([
        (
            "content-type".into(),
            inline_mime
                .unwrap_or(if archive {
                    "application/gzip"
                } else {
                    "application/octet-stream"
                })
                .into(),
        ),
        ("content-length".into(), size.to_string()),
        (
            "content-disposition".into(),
            format!(
                "{disposition}; filename=\"{fallback}\"; filename*=UTF-8''{}",
                encode(filename)
            ),
        ),
        ("x-file-path".into(), encode(path)),
    ]);
    if inline_mime.is_some() {
        headers.insert("cache-control".into(), "private, no-store".into());
        headers.insert("x-content-type-options".into(), "nosniff".into());
    }
    headers
}
pub(crate) async fn download(
    host: &HostConfig,
    checkout: &Checkout,
    params: &Value,
) -> Result<Download> {
    let requested = required_path(params, false)?;
    if matches!(host, HostConfig::Ssh { .. }) {
        return remote::download(host, checkout, &requested, params["inline"] == true).await;
    }
    let (root, file) = target(&checkout.path, &requested, false).await?;
    let meta = fs::metadata(&file).await?;
    let path = display(
        file.strip_prefix(root)
            .map_err(|_| Error::Invalid("path escaped root".into()))?,
    );
    let mut filename = file
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let (body, size, temporary) = if meta.is_dir() {
        filename.push_str(".tar.gz");
        let parent = display(
            file.parent()
                .ok_or_else(|| Error::Invalid("missing parent".into()))?,
        );
        let name = file.file_name().unwrap_or_default().to_string_lossy();
        let (temp, size) =
            process::spool(host, &["tar", "-czf", "-", "-C", &parent, "--", &name], 120).await?;
        (fs::File::from_std(temp.reopen()?), size, Some(temp))
    } else if meta.is_file() {
        (fs::File::open(&file).await?, meta.len(), None)
    } else {
        return Err(Error::Invalid(
            "only regular files and directories can be downloaded".into(),
        ));
    };
    Ok(Download {
        headers: headers(
            &path,
            &filename,
            size,
            meta.is_dir(),
            params["inline"] == true,
        ),
        filename,
        path,
        size,
        body,
        _temporary: temporary,
    })
}
