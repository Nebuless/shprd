use crate::{
    Checkout, Error, GIT_DIFF_MAX_BYTES, HostConfig, Range, Result, WorkspaceService, files, path,
    process::{self, Output, quote},
    required_path, string,
};
use serde_json::{Value, json};
use std::collections::HashMap;
async fn run(host: &HostConfig, root: &str, args: &[&str]) -> Result<Output> {
    let mut argv = vec![
        "git",
        "--literal-pathspecs",
        "-C",
        root,
        "-c",
        "core.quotepath=false",
    ];
    argv.extend_from_slice(args);
    let output = process::run(host, &argv, None, 10, 16 * 1024 * 1024).await?;
    if output.truncated {
        return Err(Error::Process(
            "git metadata exceeds 16 MiB safety limit".into(),
        ));
    }
    Ok(output)
}
pub(crate) async fn root(host: &HostConfig, checkout: &str) -> Result<String> {
    Ok(run(host, checkout, &["rev-parse", "--show-toplevel"])
        .await?
        .checked()?
        .text()
        .trim()
        .to_owned())
}
async fn head(host: &HostConfig, root: &str) -> Result<bool> {
    Ok(run(
        host,
        root,
        &["rev-parse", "--verify", "--quiet", "HEAD^{commit}"],
    )
    .await?
    .code
        == 0)
}
fn conflicted(x: u8, y: u8) -> bool {
    x == b'U' || y == b'U' || (x == b'A' && y == b'A') || (x == b'D' && y == b'D')
}
fn label(code: u8) -> &'static str {
    match code {
        b'A' => "added",
        b'D' => "deleted",
        b'R' => "renamed",
        b'C' => "copied",
        b'T' => "type changed",
        _ => "modified",
    }
}
fn entry(path: &str, old: Option<&str>, kind: &str, status: &str) -> Value {
    let mut value = json!({"path":path,"kind":kind,"status":status});
    if let Some(old) = old {
        value["old_path"] = json!(old);
    }
    value
}
fn status_entries(bytes: &[u8]) -> Vec<Value> {
    let text = String::from_utf8_lossy(bytes);
    let mut fields = text.split('\0');
    let mut entries = Vec::new();
    while let Some(record) = fields.next() {
        if record.len() < 4 {
            continue;
        }
        let x = record.as_bytes()[0];
        let y = record.as_bytes()[1];
        let path = &record[3..];
        let old = if matches!(x, b'R' | b'C') || matches!(y, b'R' | b'C') {
            fields.next()
        } else {
            None
        };
        if x == b'?' && y == b'?' {
            entries.push(entry(path, None, "untracked", "untracked"));
        } else if conflicted(x, y) {
            entries.push(entry(path, old, "conflicted", "conflicted"));
        } else {
            if x != b' ' {
                entries.push(entry(path, old, "staged", label(x)));
            }
            if y != b' ' {
                entries.push(entry(path, old, "unstaged", label(y)));
            }
        }
    }
    entries.sort_by(|a, b| {
        string(a, "path")
            .to_lowercase()
            .cmp(&string(b, "path").to_lowercase())
            .then_with(|| string(a, "kind").cmp(string(b, "kind")))
    });
    entries
}
fn branch_entries(bytes: &[u8], kind: &str) -> Vec<Value> {
    let text = String::from_utf8_lossy(bytes);
    let mut fields = text.split('\0');
    let mut entries = Vec::new();
    while let Some(status) = fields.next() {
        if status.is_empty() {
            continue;
        }
        let Some(first) = fields.next() else { break };
        let code = status.as_bytes()[0];
        let (path, old) = if matches!(code, b'R' | b'C') {
            (fields.next().unwrap_or(first), Some(first))
        } else {
            (first, None)
        };
        entries.push(entry(path, old, kind, label(code)));
    }
    entries.sort_by_key(|e| string(e, "path").to_lowercase());
    entries
}
fn counts(entries: &[Value], all: bool) -> Value {
    let mut result = json!({"staged":0,"unstaged":0,"untracked":0,"conflicted":0});
    if all {
        result["branch"] = json!(0);
        result["last-step"] = json!(0);
    }
    for entry in entries {
        let key = string(entry, "kind");
        result[key] = json!(result[key].as_u64().unwrap_or(0) + 1);
    }
    result
}
async fn working(host: &HostConfig, root: &str) -> Result<Vec<Value>> {
    Ok(status_entries(
        &run(
            host,
            root,
            &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        )
        .await?
        .checked()?
        .stdout,
    ))
}
async fn fingerprint(host: &HostConfig, root: &str, path: &str) -> Result<Option<(u64, f64)>> {
    match host {
        HostConfig::Local => {
            let target = std::path::Path::new(root).join(path);
            match tokio::fs::metadata(target).await {
                Ok(meta) if meta.is_file() => Ok(Some((
                    meta.len(),
                    (files::mtime(&meta) / 1000.0).trunc() * 1000.0,
                ))),
                Ok(_) => Ok(None),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e.into()),
            }
        }
        HostConfig::Ssh { .. } => {
            let script = format!(
                "cd -- {} || exit; f={}; [ -f \"$f\" ] || exit 0; stat -Lc '%s %Y' -- \"$f\" 2>/dev/null || stat -Lf '%z %m' \"$f\"",
                quote(root),
                quote(&format!("./{path}"))
            );
            let output = process::run(host, &["sh", "-c", &script], None, 10, 4096)
                .await?
                .checked()?;
            let text = output.text();
            let mut values = text.split_whitespace();
            Ok(values
                .next()
                .and_then(|s| s.parse().ok())
                .zip(values.next().and_then(|s| s.parse::<f64>().ok()))
                .map(|(size, time)| (size, time * 1000.0)))
        }
    }
}
fn stale(path: &str) -> Error {
    Error::Stale(format!(
        "{path} changed since the last refresh; refresh Changes and try again"
    ))
}
async fn file_action(host: &HostConfig, root: &str, params: &Value) -> Result<Value> {
    let action = string(params, "action");
    if !["stage", "unstage", "discard_unstaged", "delete_untracked"].contains(&action) {
        return Err(Error::Invalid(
            "git.file_action requires a valid action".into(),
        ));
    }
    let path = required_path(params, false)?;
    let old = path_value(params, "old_path")?;
    let output = run(
        host,
        root,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--",
            &path,
        ],
    )
    .await?
    .checked()?;
    let Some(record) = output.stdout.split(|b| *b == 0).find(|s| s.len() >= 3) else {
        return Err(stale(&path));
    };
    let x = record[0];
    let y = record[1];
    let untracked = x == b'?' && y == b'?';
    let conflict = conflicted(x, y);
    let allowed = match action {
        "stage" => untracked || conflict || y != b' ',
        "unstage" => !untracked && !conflict && x != b' ',
        "discard_unstaged" => !untracked && !conflict && y != b' ',
        "delete_untracked" => untracked,
        _ => false,
    };
    if !allowed {
        return Err(stale(&path));
    }
    if action == "discard_unstaged" || action == "delete_untracked" {
        let current = fingerprint(host, root, &path).await?;
        let expected = params["mtime_ms"]
            .as_f64()
            .filter(|n| n.is_finite() && *n > 0.0)
            .zip(params["size"].as_u64());
        match (expected, current) {
            (Some((time, size)), Some((actual_size, actual_time)))
                if size == actual_size
                    && (time / 1000.0).trunc() == (actual_time / 1000.0).trunc() => {}
            (None, None) => {}
            _ => return Err(stale(&path)),
        }
    }
    if action == "delete_untracked" {
        let script = format!(
            "set -eu\ncd -- {}\nf={}\nif [ -d \"$f\" ] && [ ! -L \"$f\" ]; then printf '%s\\n' 'delete_untracked supports files only; recursive directory deletion is not supported' >&2; exit 15; fi\nexec git --literal-pathspecs clean -f -- {}",
            quote(root),
            quote(&format!("./{path}")),
            quote(&path)
        );
        process::run(host, &["sh", "-c", &script], None, 10, 16384)
            .await?
            .checked()?;
    } else {
        let mut args = match action {
            "stage" => vec!["add"],
            "unstage" => {
                if head(host, root).await? {
                    vec!["reset", "-q", "HEAD"]
                } else {
                    vec!["rm", "-q", "--cached"]
                }
            }
            "discard_unstaged" => vec!["checkout"],
            _ => return Err(Error::Invalid("invalid action".into())),
        };
        args.push("--");
        if action == "unstage" && !old.is_empty() && old != path {
            args.push(&old);
        }
        args.push(&path);
        run(host, root, &args).await?.checked()?;
    }
    let mut result = json!({"action":action,"path":path});
    if !old.is_empty() {
        result["old_path"] = json!(old);
    }
    Ok(result)
}
fn path_value(params: &Value, key: &str) -> Result<String> {
    path(string(params, key), false)
}
async fn repo_action(host: &HostConfig, root: &str, params: &Value) -> Result<Value> {
    let action = string(params, "action");
    let before = counts(&working(host, root).await?, false);
    let args = match action {
        "stage_all" => vec!["add", "-A"],
        "unstage_all" => {
            if head(host, root).await? {
                vec!["reset", "-q"]
            } else {
                vec!["rm", "-r", "-q", "--cached", "."]
            }
        }
        "discard_all_unstaged" | "delete_all_untracked" => {
            let key = if action == "discard_all_unstaged" {
                "unstaged"
            } else {
                "untracked"
            };
            if let Some(expected) = params["expected_counts"][key].as_u64()
                && before[key] != expected
            {
                return Err(stale("the working tree"));
            }
            if action == "discard_all_unstaged" {
                if before["conflicted"].as_u64().unwrap_or(0) > 0 {
                    return Err(Error::Invalid(
                        "resolve conflicted files before discarding all unstaged changes".into(),
                    ));
                }
                vec!["checkout", "--", "."]
            } else {
                vec!["clean", "-fd"]
            }
        }
        _ => {
            return Err(Error::Invalid(
                "git.repo_action requires a valid action".into(),
            ));
        }
    };
    run(host, root, &args).await?.checked()?;
    Ok(json!({"action":action,"counts":counts(&working(host,root).await?,false)}))
}

async fn main_base(host: &HostConfig, root: &str) -> Result<String> {
    for name in [
        "main",
        "refs/heads/main",
        "origin/main",
        "refs/remotes/origin/main",
    ] {
        if run(
            host,
            root,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{name}^{{commit}}"),
            ],
        )
        .await?
        .code
            == 0
        {
            return Ok(name.into());
        }
    }
    Err(Error::Invalid("main branch was not found".into()))
}
fn mode(params: &Value) -> &str {
    match string(params, "mode") {
        "branch-main" => "branch-main",
        "last-step" => "last-step",
        _ => "working",
    }
}
fn kind<'a>(mode: &'a str, params: &'a Value) -> &'a str {
    match mode {
        "branch-main" => "branch",
        "last-step" => "last-step",
        _ => match string(params, "kind") {
            "staged" => "staged",
            "untracked" => "untracked",
            "conflicted" => "conflicted",
            _ => "unstaged",
        },
    }
}
async fn tree_exists(host: &HostConfig, root: &str, tree: &str) -> Result<bool> {
    let output = run(host, root, &["cat-file", "-t", tree]).await?;
    Ok(output.code == 0 && output.text().trim() == "tree")
}
async fn range(
    service: &WorkspaceService,
    checkout: &Checkout,
    root: &str,
    snapshot: Option<&str>,
) -> Result<Option<Range>> {
    let mut store = service.baselines.lock().await;
    let range = if let Some(id) = snapshot {
        store
            .snapshots
            .iter()
            .find(|(sid, workspace, range)| {
                sid == id && workspace == &checkout.workspace_id && range.root == root
            })
            .map(|(_, _, r)| r.clone())
            .ok_or_else(|| Error::Stale("last-step snapshot expired; refresh Changes".into()))?
    } else {
        let Some(range) = store
            .completed
            .get(&checkout.workspace_id)
            .filter(|r| r.root == root)
            .cloned()
        else {
            return Ok(None);
        };
        range
    };
    if !tree_exists(&service.host, root, &range.baseline).await?
        || !tree_exists(&service.host, root, &range.current).await?
    {
        store.completed.remove(&checkout.workspace_id);
        store
            .snapshots
            .retain(|(_, workspace, _)| workspace != &checkout.workspace_id);
        if snapshot.is_some() {
            return Err(Error::Stale(
                "last-step snapshot expired; refresh Changes".into(),
            ));
        }
        return Ok(None);
    }
    Ok(Some(range))
}
pub(crate) async fn snapshot(host: &HostConfig, root: &str) -> Result<String> {
    // A private throwaway index preserves staged content and never changes HEAD.
    let script = format!(
        "set -eu\nindex_dir=$(mktemp -d /tmp/shprd-git-index.XXXXXX)\ntrap 'rm -rf -- \"$index_dir\"' EXIT HUP INT TERM\nexport GIT_INDEX_FILE=\"$index_dir/index\"\ncd -- {}\nif git rev-parse --verify --quiet 'HEAD^{{commit}}' >/dev/null; then git read-tree HEAD; else git read-tree --empty; fi\ngit add -A\ngit write-tree\n",
        quote(root)
    );
    let output = process::run(host, &["sh", "-c", &script], None, 10, 4096)
        .await?
        .checked()?;
    let tree = output.text().trim().to_owned();
    if !(40..=64).contains(&tree.len()) || !tree.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::Process("invalid git tree object".into()));
    }
    Ok(tree)
}
fn numstat(bytes: &[u8]) -> HashMap<String, (u64, u64)> {
    let text = String::from_utf8_lossy(bytes);
    let mut records = text.split('\0');
    let mut result = HashMap::new();
    while let Some(record) = records.next() {
        let mut fields = record.splitn(3, '\t');
        let Some(add) = fields.next() else { continue };
        let Some(del) = fields.next() else { continue };
        let Some(path) = fields.next() else { continue };
        let path = if path.is_empty() {
            let _old = records.next();
            records.next().unwrap_or("")
        } else {
            path
        };
        if !path.is_empty() {
            result.insert(
                path.to_owned(),
                (add.parse().unwrap_or(0), del.parse().unwrap_or(0)),
            );
        }
    }
    result
}
async fn enrich(
    host: &HostConfig,
    root: &str,
    entries: &mut [Value],
    mode: &str,
    range_args: &[String],
) -> Result<()> {
    let mut stats = HashMap::new();
    if mode == "working" {
        for (kind, args) in [
            (
                "staged",
                vec![
                    "diff",
                    "--cached",
                    "--numstat",
                    "-z",
                    "--find-renames",
                    "--no-ext-diff",
                ],
            ),
            (
                "unstaged",
                vec!["diff", "--numstat", "-z", "--find-renames", "--no-ext-diff"],
            ),
        ] {
            let output = run(host, root, &args).await?;
            if output.code == 0 {
                for (path, counts) in numstat(&output.stdout) {
                    stats.insert((kind.to_owned(), path.clone()), counts);
                    if kind == "unstaged" {
                        stats.insert(("conflicted".into(), path), counts);
                    }
                }
            }
        }
        for entry in entries.iter() {
            if entry["kind"] == "untracked" {
                let path = string(entry, "path");
                let out = run(
                    host,
                    root,
                    &[
                        "diff",
                        "--no-ext-diff",
                        "--no-index",
                        "--numstat",
                        "-z",
                        "--",
                        "/dev/null",
                        path,
                    ],
                )
                .await?;
                if out.code <= 1 {
                    let total = numstat(&out.stdout)
                        .values()
                        .fold((0, 0), |(a, d), (x, y)| (a + x, d + y));
                    stats.insert(("untracked".into(), path.into()), total);
                }
            }
        }
    } else {
        let mut args = vec!["diff", "--numstat", "-z", "--find-renames", "--no-ext-diff"];
        args.extend(range_args.iter().map(String::as_str));
        let out = run(host, root, &args).await?.checked()?;
        for (path, counts) in numstat(&out.stdout) {
            stats.insert(
                (
                    if mode == "last-step" {
                        "last-step"
                    } else {
                        "branch"
                    }
                    .into(),
                    path,
                ),
                counts,
            );
        }
    }
    for batch in entries.chunks_mut(100) {
        let paths: Vec<_> = batch.iter().map(|e| string(e, "path").to_owned()).collect();
        let mut args = vec!["check-attr", "-z", "linguist-generated", "--"];
        args.extend(paths.iter().map(String::as_str));
        let output = run(host, root, &args).await?;
        let text = output.text();
        let attrs: Vec<_> = text.split('\0').collect();
        for entry in batch {
            let path = string(entry, "path").to_owned();
            if let Some((a, d)) = stats.get(&(string(entry, "kind").into(), path.clone())) {
                entry["additions"] = json!(a);
                entry["deletions"] = json!(d);
            }
            if mode == "working"
                && let Some((size, time)) = fingerprint(host, root, &path).await?
            {
                entry["size"] = json!(size);
                entry["mtime_ms"] = json!(time);
            }
            if attrs.as_chunks::<3>().0.iter().any(|v| {
                v[0] == path
                    && v[1] == "linguist-generated"
                    && matches!(v[2].to_ascii_lowercase().as_str(), "set" | "true")
            }) {
                entry["generated"] = json!(true);
            }
        }
    }
    Ok(())
}
async fn summary(
    service: &WorkspaceService,
    checkout: &Checkout,
    root: &str,
    params: &Value,
) -> Result<Value> {
    let mode = mode(params);
    let mut base = None;
    let mut snapshot_id = None;
    let mut range_args = Vec::new();
    let mut available = true;
    let mut entries = match mode {
        "branch-main" => {
            let main = main_base(&service.host, root).await?;
            range_args.push(format!("{main}...HEAD"));
            base = Some(main);
            branch_entries(
                &run(
                    &service.host,
                    root,
                    &[
                        "diff",
                        "--name-status",
                        "-z",
                        "--find-renames",
                        &range_args[0],
                    ],
                )
                .await?
                .checked()?
                .stdout,
                "branch",
            )
        }
        "last-step" => {
            if let Some(range) = range(service, checkout, root, None).await? {
                base = Some(range.baseline.clone());
                range_args.extend([range.baseline.clone(), range.current.clone()]);
                let entries = branch_entries(
                    &run(
                        &service.host,
                        root,
                        &[
                            "diff",
                            "--name-status",
                            "-z",
                            "--find-renames",
                            &range.baseline,
                            &range.current,
                        ],
                    )
                    .await?
                    .checked()?
                    .stdout,
                    "last-step",
                );
                let mut store = service.baselines.lock().await;
                store.serial += 1;
                let id = format!("snapshot-{}", store.serial);
                store
                    .snapshots
                    .push_back((id.clone(), checkout.workspace_id.clone(), range));
                while store.snapshots.len() > 64 {
                    store.snapshots.pop_front();
                }
                snapshot_id = Some(id);
                entries
            } else {
                available = false;
                Vec::new()
            }
        }
        _ => working(&service.host, root).await?,
    };
    if available {
        enrich(&service.host, root, &mut entries, mode, &range_args).await?;
    }
    let mut value = json!({"workspace_id":checkout.workspace_id,"repo_name":checkout.repo_name,"root":root,"mode":mode,"baseline_available":available,"counts":counts(&entries,true),"entries":entries});
    if let Some(base) = base {
        value["base"] = json!(base);
    }
    if let Some(id) = snapshot_id {
        value["snapshot_id"] = json!(id);
    }
    Ok(value)
}
async fn diff_file(
    service: &WorkspaceService,
    checkout: &Checkout,
    root: &str,
    params: &Value,
) -> Result<Value> {
    let path = required_path(params, false)?;
    let old = path_value(params, "old_path")?;
    let mode = mode(params);
    let kind = kind(mode, params);
    let mut range_args = Vec::new();
    if mode == "branch-main" {
        range_args.push(format!("{}...HEAD", main_base(&service.host, root).await?));
    }
    if mode == "last-step" {
        let id = string(params, "snapshot_id");
        if id.is_empty() {
            return Err(Error::Stale(
                "last-step diff requires a fresh summary snapshot".into(),
            ));
        }
        if let Some(range) = range(service, checkout, root, Some(id)).await? {
            range_args.extend([range.baseline, range.current]);
        }
    }
    let mut args = vec![
        "git",
        "--literal-pathspecs",
        "-C",
        root,
        "-c",
        "core.quotepath=false",
        "diff",
        "--no-ext-diff",
    ];
    match kind {
        "staged" => args.push("--cached"),
        "untracked" => args.push("--no-index"),
        "conflicted" => args.push("--cc"),
        "branch" | "last-step" => args.push("--find-renames"),
        _ => {}
    }
    args.extend(range_args.iter().map(String::as_str));
    args.push("--");
    if kind == "untracked" {
        args.push("/dev/null");
    } else if !old.is_empty() && old != path {
        args.push(&old);
    }
    args.push(&path);
    let output = process::run(&service.host, &args, None, 10, GIT_DIFF_MAX_BYTES).await?;
    if output.code != 0 && !(kind == "untracked" && output.code == 1) {
        return Err(output.error());
    }
    Ok(
        json!({"workspace_id":checkout.workspace_id,"root":root,"path":path,"kind":kind,"diff":output.text(),"truncated":output.truncated}),
    )
}
async fn status(host: &HostConfig, root: &str) -> Result<Value> {
    let out = run(host, root, &["status", "--porcelain=v1", "--branch", "-z"]).await;
    let mut value = json!({"ahead":0,"behind":0,"staged":0,"unstaged":0,"untracked":0,"conflicted":0,"dirty":false});
    let output = match out.and_then(Output::checked) {
        Ok(out) => out,
        Err(e) => {
            value["error"] = json!(e.to_string().chars().take(300).collect::<String>());
            return Ok(value);
        }
    };
    let text = output.text();
    let (header, records) = text.split_once('\0').unwrap_or((&text, ""));
    let header = header.strip_prefix("## ").unwrap_or(header);
    let (branch, tracking) = header.split_once(" [").unwrap_or((header, ""));
    let (branch, upstream) = branch
        .split_once("...")
        .map_or((branch, None), |(b, u)| (b, Some(u)));
    value["branch"] = json!(if branch == "HEAD (no branch)" {
        "detached"
    } else {
        branch.strip_prefix("No commits yet on ").unwrap_or(branch)
    });
    if let Some(upstream) = upstream {
        value["upstream"] = json!(upstream);
    }
    for item in tracking.trim_end_matches(']').split(", ") {
        if let Some((key, number)) = item.split_once(' ')
            && matches!(key, "ahead" | "behind")
        {
            value[key] = json!(number.parse::<u64>().unwrap_or(0));
        }
    }
    let counts = counts(&status_entries(records.as_bytes()), false);
    for key in ["staged", "unstaged", "untracked", "conflicted"] {
        value[key] = counts[key].clone();
    }
    value["dirty"] = json!(
        ["staged", "unstaged", "untracked", "conflicted"]
            .iter()
            .any(|k| value[k].as_u64().unwrap_or(0) > 0)
    );
    Ok(value)
}
pub(crate) async fn dispatch(
    service: &WorkspaceService,
    checkout: &Checkout,
    method: &str,
    params: &Value,
) -> Result<Value> {
    if method == "git.status" {
        return status(&service.host, &checkout.path).await;
    }
    let root = root(&service.host, &checkout.path).await?;
    let mut result = match method {
        "git.diff_summary" => return summary(service, checkout, &root, params).await,
        "git.diff_file" => return diff_file(service, checkout, &root, params).await,
        "git.file_action" => file_action(&service.host, &root, params).await?,
        "git.repo_action" => repo_action(&service.host, &root, params).await?,
        "git.pull" => {
            let out = process::run(
                &service.host,
                &[
                    "env",
                    "GIT_TERMINAL_PROMPT=0",
                    "git",
                    "-C",
                    &root,
                    "-c",
                    "core.quotepath=false",
                    "pull",
                    "--ff-only",
                ],
                None,
                120,
                1024 * 1024,
            )
            .await?
            .checked()?;
            json!({"stdout":out.text().trim(),"stderr":out.stderr.trim()})
        }
        _ => return Err(Error::Invalid("unknown git method".into())),
    };
    result["workspace_id"] = json!(checkout.workspace_id);
    result["root"] = json!(root);
    Ok(result)
}
