use crate::{
    Checkout, Error, HostConfig, Result,
    files::{self, Download},
    path, process, required_path, string,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use tokio::{fs, io::AsyncRead};

fn decode(value: &str) -> Result<String> {
    String::from_utf8(
        STANDARD
            .decode(value)
            .map_err(|e| Error::Process(e.to_string()))?,
    )
    .map_err(|e| Error::Process(e.to_string()))
}
const SETUP: &str = r#"set -euo pipefail
root_real=$(cd -- "$1" && pwd -P)
request="$2"
target="$root_real/$request"
if [ "$3" = preview ]; then case "$request" in /*) target="$request";; esac; fi
target_real=$(realpath -- "$target")
if [ "$3" != preview ] || [[ "$request" != /* ]]; then
 case "$target_real/" in "${root_real%/}/"*) ;; *) echo 'file explorer path escaped the workspace checkout' >&2; exit 13;; esac
fi
enc() { printf '%s' "$1" | base64 | tr -d '\n'; }
size_of() { stat -Lc '%s' -- "$1" 2>/dev/null || stat -Lf '%z' "$1"; }
mtime_of() { stat -Lc '%Y' -- "$1" 2>/dev/null || stat -Lf '%m' "$1"; }
rel="${target_real#"$root_real"/}"
"#;
fn script(body: &str) -> String {
    format!("{SETUP}\n{body}").replace("\n+", "\n")
}
async fn run(
    host: &HostConfig,
    root: &str,
    path: &str,
    mode: &str,
    body: &str,
    limit: usize,
) -> Result<process::Output> {
    process::run(
        host,
        &["bash", "-c", &script(body), "shprd", root, path, mode],
        None,
        10,
        limit,
    )
    .await?
    .checked()
}
pub(crate) async fn regular_file(host: &HostConfig, root: &str, path: &str) -> Result<bool> {
    let output = process::run(
        host,
        &[
            "bash",
            "-c",
            &script("[ -f \"$target_real\" ]"),
            "shprd",
            root,
            path,
            "preview",
        ],
        None,
        10,
        1024,
    )
    .await?;
    if matches!(output.code, 0 | 1 | 13) {
        Ok(output.code == 0)
    } else {
        Err(output.error())
    }
}
pub(crate) async fn dispatch(
    host: &HostConfig,
    checkout: &Checkout,
    method: &str,
    params: &Value,
) -> Result<Value> {
    match method {
        "file.list" => {
            let relative = path(string(params, "path"), false)?;
            let body = format!(
                r#"printf 'ROOT\t%s\n' "$(enc "$root_real")"
count=0
while IFS= read -r -d '' p; do
 name="${{p##*/}}"
 if [ {} != 1 ] && [[ "$name" = .* ]]; then continue; fi
 count=$((count+1))
 if [ "$count" -gt 1000 ]; then printf 'TRUNCATED\n'; break; fi
 if [ -L "$p" ]; then type=symlink; elif [ -d "$p" ]; then type=directory; else type=file; fi
 printf 'ENTRY\t%s\t%s\t%s\t%s\n' "$type" "$(size_of "$p" || printf 0)" "$(mtime_of "$p" || printf 0)" "$(enc "$name")"
done < <(find "$target_real" -mindepth 1 -maxdepth 1 -print0)
"#,
                if params["show_hidden"] == true { 1 } else { 0 }
            );
            let output = run(
                host,
                &checkout.path,
                &relative,
                "explorer",
                &body,
                2 * 1024 * 1024,
            )
            .await?;
            let mut root = String::new();
            let mut entries = Vec::new();
            let mut truncated = false;
            for line in output.text().lines() {
                let fields: Vec<_> = line.split('\t').collect();
                match fields.as_slice() {
                    ["ROOT", encoded] => root = decode(encoded)?,
                    ["TRUNCATED"] => truncated = true,
                    ["ENTRY", ty, size, mtime, encoded] => {
                        let name = decode(encoded)?;
                        entries.push(json!({"name":name,"path":if relative.is_empty(){name.clone()}else{format!("{relative}/{name}")},"type":ty,"size":size.parse::<u64>().unwrap_or(0),"mtime_ms":mtime.parse::<f64>().unwrap_or(0.0)*1000.0,"hidden":name.starts_with('.')}));
                    }
                    _ => {}
                }
            }
            if root.is_empty() || output.truncated {
                return Err(Error::Process("invalid file list response".into()));
            }
            files::sort_entries(&mut entries);
            files::ignored(
                host,
                &if relative.is_empty() {
                    root.clone()
                } else {
                    format!("{root}/{relative}")
                },
                &mut entries,
            )
            .await;
            Ok(json!({"root":root,"path":relative,"entries":entries,"truncated":truncated}))
        }
        "file.read" => {
            let requested = required_path(params, true)?;
            let body = r#"[ -f "$target_real" ] || { echo 'only regular files can be previewed' >&2; exit 14; }
size=$(size_of "$target_real")
case "$request" in /*) rel="$target_real";; esac
limit=524288
case "$(printf '%s' "$rel" | tr '[:upper:]' '[:lower:]')" in *.png|*.jpg|*.jpeg|*.gif|*.webp|*.bmp|*.ico|*.avif) if [ "$size" -le 5242880 ]; then limit=5242880; fi;; esac
printf 'META\t%s\t%s\t%s\t%s\n' "$(enc "$root_real")" "$size" "$(mtime_of "$target_real")" "$(enc "$rel")"
head -c "$((limit+1))" -- "$target_real" | base64 | tr -d '\n'
"#;
            let output = run(
                host,
                &checkout.path,
                &requested,
                "preview",
                body,
                8 * 1024 * 1024,
            )
            .await?;
            let text = output.text();
            let (meta, encoded) = text
                .split_once('\n')
                .ok_or_else(|| Error::Process("invalid file preview response".into()))?;
            let fields: Vec<_> = meta.split('\t').collect();
            let ["META", root, size, mtime, path] = fields.as_slice() else {
                return Err(Error::Process("invalid preview metadata".into()));
            };
            let root = decode(root)?;
            let path = decode(path)?;
            let size = size
                .parse::<u64>()
                .map_err(|e| Error::Process(e.to_string()))?;
            let mtime = mtime
                .parse::<f64>()
                .map_err(|e| Error::Process(e.to_string()))?
                * 1000.0;
            let mut bytes = STANDARD
                .decode(encoded.trim())
                .map_err(|e| Error::Process(e.to_string()))?;
            let limit =
                if files::mime(&path).is_some() && size <= crate::PREVIEW_IMAGE_MAX_BYTES as u64 {
                    crate::PREVIEW_IMAGE_MAX_BYTES
                } else {
                    crate::PREVIEW_MAX_BYTES
                };
            let truncated = bytes.len() > limit || size > limit as u64;
            bytes.truncate(limit);
            let mut value = files::decode(&bytes, truncated, &path);
            value["root"] = json!(root);
            value["path"] = json!(path);
            value["size"] = json!(size);
            value["mtime_ms"] = json!(mtime);
            value["truncated"] = json!(truncated);
            Ok(value)
        }
        "file.delete" => {
            let requested = required_path(params, false)?;
            let command=r#"set -euo pipefail
root_real=$(cd -- "$1" && pwd -P)
target="$root_real/$2"
parent_real=$(cd -- "$(dirname -- "$target")" && pwd -P)
case "$parent_real/" in "${root_real%/}/"*) ;; *) echo 'file explorer path escaped the workspace checkout' >&2; exit 13;; esac
if [ -L "$target" ]; then type=symlink; elif [ -d "$target" ]; then type=directory; elif [ -e "$target" ]; then type=file; else echo 'file does not exist' >&2; exit 14; fi
rm -r -- "$target"
printf '%s' "$type"
"#.replace("\n+","\n");
            let output = process::run(
                host,
                &["bash", "-c", &command, "shprd", &checkout.path, &requested],
                None,
                120,
                1024,
            )
            .await?
            .checked()?;
            Ok(json!({"path":requested,"type":output.text()}))
        }
        _ => Err(Error::Invalid("unknown remote file operation".into())),
    }
}
pub(crate) async fn upload<R: AsyncRead + Unpin>(
    host: &HostConfig,
    checkout: &Checkout,
    directory: &str,
    name: &str,
    body: &mut R,
) -> Result<Value> {
    let spool = tempfile::NamedTempFile::new()?;
    let mut writer = fs::File::from_std(spool.reopen()?);
    let size = tokio::time::timeout(
        std::time::Duration::from_secs(120),
        tokio::io::copy(body, &mut writer),
    )
    .await
    .map_err(|_| Error::Timeout)??;
    writer.sync_all().await?;
    let body = r#"dir_real="$target_real"
[ -d "$dir_real" ] || { echo 'upload target is not a directory' >&2; exit 14; }
target="$dir_real/$4"
if [ -L "$target" ] || [ -d "$target" ]; then echo 'cannot overwrite a directory or symlink' >&2; exit 15; fi
overwritten=0
before=''
if [ -e "$target" ]; then [ -f "$target" ] || exit 15; overwritten=1; before="$(size_of "$target") $(mtime_of "$target")"; fi
tmp=$(mktemp "$dir_real/.shprd-upload.XXXXXX")
trap 'rm -f -- "$tmp"' EXIT HUP INT TERM
cat > "$tmp"
[ "$(size_of "$tmp")" = "$5" ] || { echo 'incomplete upload' >&2; exit 16; }
if [ -L "$target" ] || [ -d "$target" ]; then echo 'upload target changed during upload' >&2; exit 17; fi
if [ "$overwritten" = 1 ]; then
 [ -f "$target" ] && [ "$before" = "$(size_of "$target") $(mtime_of "$target")" ] || exit 17
 chmod --reference="$target" "$tmp" 2>/dev/null || chmod "$(stat -f %Lp "$target")" "$tmp"
 mv -f -- "$tmp" "$target"
else
 ln -- "$tmp" "$target"
fi
printf 'META\t%s\t%s\n' "$(enc "${target#"$root_real"/}")" "$overwritten"
"#;
    let mut reader = fs::File::from_std(spool.reopen()?);
    let output = process::stream_input(
        host,
        &[
            "bash",
            "-c",
            &script(body),
            "shprd",
            &checkout.path,
            directory,
            "explorer",
            name,
            &size.to_string(),
        ],
        &mut reader,
    )
    .await?
    .checked()?;
    let text = output.text();
    let fields: Vec<_> = text.trim().split('\t').collect();
    let ["META", path, overwritten] = fields.as_slice() else {
        return Err(Error::Process("invalid file upload response".into()));
    };
    Ok(
        json!({"workspace_id":checkout.workspace_id,"directory":directory,"filename":name,"path":decode(path)?,"size":size,"overwritten":*overwritten=="1"}),
    )
}
pub(crate) async fn download(
    host: &HostConfig,
    checkout: &Checkout,
    requested: &str,
    inline: bool,
) -> Result<Download> {
    let body = r#"if [ -d "$target_real" ]; then type=directory; elif [ -f "$target_real" ]; then type=file; else echo 'only regular files and directories can be downloaded' >&2; exit 14; fi
printf 'META\t%s\t%s\t%s\n' "$type" "$(enc "$rel")" "$(enc "$(basename -- "$target_real")")"
"#;
    let output = run(host, &checkout.path, requested, "explorer", body, 16384).await?;
    let text = output.text();
    let fields: Vec<_> = text.trim().split('\t').collect();
    let ["META", kind, path, name] = fields.as_slice() else {
        return Err(Error::Process("invalid download metadata".into()));
    };
    let path = decode(path)?;
    let mut filename = decode(name)?;
    let archive = *kind == "directory";
    if archive {
        filename.push_str(".tar.gz");
    }
    let body = if archive {
        "[ -d \"$target_real\" ] || exit 14; COPYFILE_DISABLE=1 tar -czf - -C \"$(dirname -- \"$target_real\")\" -- \"$(basename -- \"$target_real\")\""
    } else {
        "[ -f \"$target_real\" ] || exit 14; cat -- \"$target_real\""
    };
    let (temp, size) = process::spool(
        host,
        &[
            "bash",
            "-c",
            &script(body),
            "shprd",
            &checkout.path,
            requested,
            "explorer",
        ],
        120,
    )
    .await?;
    Ok(Download {
        headers: files::headers(&path, &filename, size, archive, inline),
        filename,
        path,
        size,
        body: fs::File::from_std(temp.reopen()?),
        _temporary: Some(temp),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;
    #[tokio::test]
    async fn remote_shell_scripts_run_against_real_files() {
        let dir = tempfile::tempdir().unwrap();
        let checkout = Checkout {
            workspace_id: "w1".into(),
            path: dir.path().to_str().unwrap().into(),
            repo_name: "Repo".into(),
        };
        let host = HostConfig::Local;
        let name = "quote' space.txt";
        fs::write(dir.path().join(name), "remote text\n")
            .await
            .unwrap();
        let listing = dispatch(&host, &checkout, "file.list", &json!({}))
            .await
            .unwrap();
        assert_eq!(listing["entries"][0]["name"], name);
        let preview = dispatch(&host, &checkout, "file.read", &json!({"path":name}))
            .await
            .unwrap();
        assert_eq!(preview["text"], "remote text\n");
        assert!(regular_file(&host, &checkout.path, name).await.unwrap());
        assert!(
            !regular_file(&host, &checkout.path, "missing")
                .await
                .unwrap()
        );
        let result = upload(&host, &checkout, "", "upload.txt", &mut &b"upload"[..])
            .await
            .unwrap();
        assert_eq!(result["overwritten"], false);
        let result = upload(&host, &checkout, "", "upload.txt", &mut &b"replaced"[..])
            .await
            .unwrap();
        assert_eq!(result["overwritten"], true);
        assert_eq!(
            fs::read(dir.path().join("upload.txt")).await.unwrap(),
            b"replaced"
        );
        let mut file = download(&host, &checkout, "upload.txt", false)
            .await
            .unwrap();
        let mut bytes = Vec::new();
        file.body.read_to_end(&mut bytes).await.unwrap();
        assert_eq!(bytes, b"replaced");
        fs::create_dir(dir.path().join("folder")).await.unwrap();
        fs::write(dir.path().join("folder/file"), "archive")
            .await
            .unwrap();
        let mut file = download(&host, &checkout, "folder", false).await.unwrap();
        let mut magic = [0; 2];
        file.body.read_exact(&mut magic).await.unwrap();
        assert_eq!(magic, [31, 139]);
        dispatch(
            &host,
            &checkout,
            "file.delete",
            &json!({"path":"upload.txt"}),
        )
        .await
        .unwrap();
        assert!(!dir.path().join("upload.txt").exists());
        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().unwrap();
            fs::write(outside.path().join("secret"), "keep")
                .await
                .unwrap();
            std::os::unix::fs::symlink(outside.path(), dir.path().join("escape")).unwrap();
            assert!(
                dispatch(
                    &host,
                    &checkout,
                    "file.read",
                    &json!({"path":"escape/secret"})
                )
                .await
                .is_err()
            );
            assert!(
                upload(&host, &checkout, "escape", "secret", &mut &b"bad"[..])
                    .await
                    .is_err()
            );
            assert!(
                dispatch(
                    &host,
                    &checkout,
                    "file.delete",
                    &json!({"path":"escape/secret"})
                )
                .await
                .is_err()
            );
            dispatch(&host, &checkout, "file.delete", &json!({"path":"escape"}))
                .await
                .unwrap();
            assert!(outside.path().join("secret").exists());
        }
    }
}
