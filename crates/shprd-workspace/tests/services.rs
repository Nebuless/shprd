use serde_json::json;
use shprd_workspace::{Checkout, WorkspaceService};
use std::{fs, process::Command};

#[tokio::test]
async fn listing_ignored_limits_and_diff_truncation() {
    let dir = repository();
    let checkout = checkout(dir.path());
    let service = WorkspaceService::default();
    fs::write(dir.path().join(".gitignore"), "ignored.log\n").unwrap();
    fs::write(dir.path().join("ignored.log"), "keep").unwrap();
    let list = call(
        &service,
        &checkout,
        "file.list",
        json!({"show_hidden":true}),
    )
    .await;
    assert!(
        list["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["name"] == "ignored.log" && e["ignored"] == true)
    );
    fs::write(dir.path().join("tracked.txt"), "line\n".repeat(150000)).unwrap();
    let diff = call(
        &service,
        &checkout,
        "git.diff_file",
        json!({"path":"tracked.txt"}),
    )
    .await;
    assert_eq!(diff["truncated"], true);
    assert!(diff["diff"].as_str().unwrap().len() <= shprd_workspace::GIT_DIFF_MAX_BYTES);
    fs::create_dir(dir.path().join("many")).unwrap();
    for i in 0..1001 {
        fs::write(dir.path().join(format!("many/file-{i}")), "").unwrap();
    }
    let list = call(&service, &checkout, "file.list", json!({"path":"many"})).await;
    assert_eq!(list["entries"].as_array().unwrap().len(), 1000);
    assert_eq!(list["truncated"], true);
}

fn repository() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-b", "main"]);
    git(dir.path(), &["config", "user.email", "test@example.com"]);
    git(dir.path(), &["config", "user.name", "SHPRD Test"]);
    fs::write(dir.path().join("tracked.txt"), "base\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "initial"]);
    dir
}
async fn call(
    service: &WorkspaceService,
    checkout: &Checkout,
    method: &str,
    params: serde_json::Value,
) -> serde_json::Value {
    service.dispatch(checkout, method, &params).await.unwrap()
}
#[tokio::test]
async fn destructive_actions_fail_closed_and_preserve_index() {
    let dir = repository();
    let checkout = checkout(dir.path());
    let service = WorkspaceService::default();
    fs::write(dir.path().join("tracked.txt"), "staged\n").unwrap();
    call(
        &service,
        &checkout,
        "git.file_action",
        json!({"action":"stage","path":"tracked.txt"}),
    )
    .await;
    fs::write(dir.path().join("tracked.txt"), "newer working content\n").unwrap();
    for params in [
        json!({"action":"discard_unstaged","path":"tracked.txt"}),
        json!({"action":"discard_unstaged","path":"tracked.txt","mtime_ms":1,"size":999}),
    ] {
        assert!(
            service
                .dispatch(&checkout, "git.file_action", &params)
                .await
                .is_err()
        );
    }
    let summary = call(&service, &checkout, "git.diff_summary", json!({})).await;
    let entry = summary["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "unstaged")
        .unwrap();
    assert_eq!(entry["additions"], 1);
    assert_eq!(entry["deletions"], 1);
    call(&service,&checkout,"git.file_action",json!({"action":"discard_unstaged","path":"tracked.txt","mtime_ms":entry["mtime_ms"],"size":entry["size"]})).await;
    assert_eq!(
        fs::read_to_string(dir.path().join("tracked.txt")).unwrap(),
        "staged\n"
    );
    assert!(git(dir.path(), &["status", "--porcelain"]).contains("M  tracked.txt"));
    fs::remove_file(dir.path().join("tracked.txt")).unwrap();
    call(
        &service,
        &checkout,
        "git.file_action",
        json!({"action":"discard_unstaged","path":"tracked.txt"}),
    )
    .await;
    assert_eq!(
        fs::read_to_string(dir.path().join("tracked.txt")).unwrap(),
        "staged\n"
    );
    assert!(
        service
            .dispatch(
                &checkout,
                "git.file_action",
                &json!({"action":"delete_untracked","path":"tracked.txt"})
            )
            .await
            .is_err()
    );
}
#[tokio::test]
async fn repository_actions_counts_conflicts_and_ignored_files() {
    let dir = repository();
    let checkout = checkout(dir.path());
    let service = WorkspaceService::default();
    fs::write(dir.path().join(".gitignore"), "keep.log\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "ignore"]);
    fs::write(dir.path().join("keep.log"), "keep").unwrap();
    fs::write(dir.path().join("new.txt"), "new").unwrap();
    fs::write(dir.path().join("tracked.txt"), "changed\n").unwrap();
    assert!(
        service
            .dispatch(
                &checkout,
                "git.repo_action",
                &json!({"action":"delete_all_untracked","expected_counts":{"untracked":0}})
            )
            .await
            .is_err()
    );
    assert!(
        service
            .dispatch(
                &checkout,
                "git.repo_action",
                &json!({"action":"discard_all_unstaged","expected_counts":{"unstaged":0}})
            )
            .await
            .is_err()
    );
    let staged = call(
        &service,
        &checkout,
        "git.repo_action",
        json!({"action":"stage_all"}),
    )
    .await;
    assert_eq!(staged["counts"]["staged"], 2);
    let unstaged = call(
        &service,
        &checkout,
        "git.repo_action",
        json!({"action":"unstage_all"}),
    )
    .await;
    assert_eq!(unstaged["counts"]["untracked"], 1);
    call(
        &service,
        &checkout,
        "git.repo_action",
        json!({"action":"delete_all_untracked","expected_counts":{"untracked":1}}),
    )
    .await;
    assert!(dir.path().join("keep.log").exists());
    assert!(!dir.path().join("new.txt").exists());
    call(
        &service,
        &checkout,
        "git.repo_action",
        json!({"action":"discard_all_unstaged","expected_counts":{"unstaged":1}}),
    )
    .await;
    assert_eq!(
        fs::read_to_string(dir.path().join("tracked.txt")).unwrap(),
        "base\n"
    );
    git(dir.path(), &["checkout", "-b", "side"]);
    fs::write(dir.path().join("tracked.txt"), "side\n").unwrap();
    git(dir.path(), &["commit", "-am", "side"]);
    git(dir.path(), &["checkout", "main"]);
    fs::write(dir.path().join("tracked.txt"), "main\n").unwrap();
    git(dir.path(), &["commit", "-am", "main"]);
    let merge = Command::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(["merge", "side"])
        .output()
        .unwrap();
    assert!(!merge.status.success());
    assert!(
        service
            .dispatch(
                &checkout,
                "git.repo_action",
                &json!({"action":"discard_all_unstaged"})
            )
            .await
            .is_err()
    );
    assert!(
        service
            .dispatch(
                &checkout,
                "git.file_action",
                &json!({"action":"unstage","path":"tracked.txt"})
            )
            .await
            .is_err()
    );
    fs::write(dir.path().join("tracked.txt"), "resolved\n").unwrap();
    call(
        &service,
        &checkout,
        "git.file_action",
        json!({"action":"stage","path":"tracked.txt"}),
    )
    .await;
    assert_eq!(
        call(&service, &checkout, "git.diff_summary", json!({})).await["counts"]["conflicted"],
        0
    );
}
#[tokio::test]
async fn literal_paths_rename_and_unborn_unstage() {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-b", "main"]);
    let checkout = checkout(dir.path());
    let service = WorkspaceService::default();
    for name in [
        "-odd name.txt",
        "colon:*.txt",
        "other.txt",
        "line\nbreak.txt",
    ] {
        fs::write(dir.path().join(name), "text\n").unwrap();
    }
    call(
        &service,
        &checkout,
        "git.file_action",
        json!({"action":"stage","path":"colon:*.txt"}),
    )
    .await;
    let summary = call(&service, &checkout, "git.diff_summary", json!({})).await;
    assert_eq!(summary["counts"]["staged"], 1);
    assert_eq!(summary["counts"]["untracked"], 3);
    call(
        &service,
        &checkout,
        "git.file_action",
        json!({"action":"unstage","path":"colon:*.txt"}),
    )
    .await;
    call(
        &service,
        &checkout,
        "git.repo_action",
        json!({"action":"stage_all"}),
    )
    .await;
    call(
        &service,
        &checkout,
        "git.repo_action",
        json!({"action":"unstage_all"}),
    )
    .await;
    assert_eq!(
        call(&service, &checkout, "git.diff_summary", json!({})).await["counts"]["untracked"],
        4
    );
    let dir = repository();
    let checkout = crate::checkout(dir.path());
    git(dir.path(), &["mv", "tracked.txt", "renamed.txt"]);
    let summary = call(&service, &checkout, "git.diff_summary", json!({})).await;
    assert_eq!(summary["entries"][0]["old_path"], "tracked.txt");
    call(
        &service,
        &checkout,
        "git.file_action",
        json!({"action":"unstage","path":"renamed.txt","old_path":"tracked.txt"}),
    )
    .await;
    assert!(!git(dir.path(), &["status", "--porcelain"]).contains("R  "));
}
#[tokio::test]
async fn immutable_last_step_branch_and_generated_diffs() {
    let dir = repository();
    let checkout = checkout(dir.path());
    let service = WorkspaceService::default();
    assert_eq!(
        call(
            &service,
            &checkout,
            "git.diff_summary",
            json!({"mode":"last-step"})
        )
        .await["baseline_available"],
        false
    );
    service.capture_workspace(&checkout).await.unwrap();
    fs::write(dir.path().join("tracked.txt"), "agent edit\n").unwrap();
    assert!(service.complete_workspace(&checkout).await.unwrap());
    assert!(!service.complete_workspace(&checkout).await.unwrap());
    let summary = call(
        &service,
        &checkout,
        "git.diff_summary",
        json!({"mode":"last-step"}),
    )
    .await;
    assert_eq!(summary["counts"]["last-step"], 1);
    assert_eq!(summary["entries"][0]["additions"], 1);
    fs::write(dir.path().join("tracked.txt"), "later edit\n").unwrap();
    let params =
        json!({"mode":"last-step","path":"tracked.txt","snapshot_id":summary["snapshot_id"]});
    let diff = call(&service, &checkout, "git.diff_file", params.clone()).await;
    assert!(diff["diff"].as_str().unwrap().contains("+agent edit"));
    assert!(!diff["diff"].as_str().unwrap().contains("later edit"));
    let other = Checkout {
        workspace_id: "other".into(),
        ..checkout.clone()
    };
    assert!(
        service
            .dispatch(&other, "git.diff_file", &params)
            .await
            .is_err()
    );
    assert!(
        WorkspaceService::default()
            .dispatch(&checkout, "git.diff_file", &params)
            .await
            .is_err()
    );
    service.invalidate_workspace("w1").await;
    assert!(
        service
            .dispatch(&checkout, "git.diff_file", &params)
            .await
            .is_err()
    );
    assert!(git(dir.path(), &["diff", "--cached"]).is_empty());
    git(dir.path(), &["checkout", "-b", "feature"]);
    git(dir.path(), &["commit", "-am", "feature"]);
    assert_eq!(
        call(
            &service,
            &checkout,
            "git.diff_summary",
            json!({"mode":"branch-main"})
        )
        .await["counts"]["branch"],
        1
    );
    fs::write(
        dir.path().join(".gitattributes"),
        "tracked.txt linguist-generated=true\n",
    )
    .unwrap();
    fs::write(dir.path().join("tracked.txt"), "generated\n").unwrap();
    let summary = call(&service, &checkout, "git.diff_summary", json!({})).await;
    assert!(
        summary["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["path"] == "tracked.txt" && e["generated"] == true)
    );
}
#[tokio::test]
async fn previews_streams_and_upload_failure_preserve_files() {
    use tokio::io::AsyncReadExt;
    let dir = tempfile::tempdir().unwrap();
    let checkout = checkout(dir.path());
    let service = WorkspaceService::default();
    fs::write(dir.path().join("binary.bin"), [0, 1, 2]).unwrap();
    fs::write(dir.path().join("image.png"), [0, 1, 2]).unwrap();
    let mut utf8 = vec![b'a'; shprd_workspace::PREVIEW_MAX_BYTES - 1];
    utf8.extend_from_slice("\u{20ac}".as_bytes());
    fs::write(dir.path().join("large.txt"), utf8).unwrap();
    let preview = call(
        &service,
        &checkout,
        "file.read",
        json!({"path":"large.txt"}),
    )
    .await;
    assert_eq!(preview["truncated"], true);
    assert_eq!(preview["binary"], false);
    assert_eq!(
        preview["text"].as_str().unwrap().len(),
        shprd_workspace::PREVIEW_MAX_BYTES - 1
    );
    assert_eq!(
        call(
            &service,
            &checkout,
            "file.read",
            json!({"path":"binary.bin"})
        )
        .await["binary"],
        true
    );
    assert_eq!(
        call(
            &service,
            &checkout,
            "file.read",
            json!({"path":"image.png"})
        )
        .await["image_data_url"],
        "data:image/png;base64,AAEC"
    );
    let mut body = &b"uploaded"[..];
    let upload = service
        .upload(
            &checkout,
            &json!({"directory":"","filename":"name.txt"}),
            &mut body,
        )
        .await
        .unwrap();
    assert_eq!(upload["overwritten"], false);
    let mut download = service
        .download(&checkout, &json!({"path":"name.txt"}))
        .await
        .unwrap();
    let mut bytes = Vec::new();
    download.body.read_to_end(&mut bytes).await.unwrap();
    assert_eq!(bytes, b"uploaded");
    assert_eq!(download.headers["content-length"], "8");
    let image = service
        .download(&checkout, &json!({"path":"image.png","inline":true}))
        .await
        .unwrap();
    assert_eq!(image.headers["x-content-type-options"], "nosniff");
    fs::create_dir(dir.path().join("folder")).unwrap();
    fs::write(dir.path().join("folder/a"), "archive").unwrap();
    let mut archive = service
        .download(&checkout, &json!({"path":"folder"}))
        .await
        .unwrap();
    let mut magic = [0; 2];
    archive.body.read_exact(&mut magic).await.unwrap();
    assert_eq!(magic, [0x1f, 0x8b]);
    assert_eq!(archive.filename, "folder.tar.gz");
    struct Broken;
    impl tokio::io::AsyncRead for Broken {
        fn poll_read(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
            _: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::task::Poll::Ready(Err(std::io::Error::other("interrupted upload")))
        }
    }
    assert!(
        service
            .upload(&checkout, &json!({"filename":"name.txt"}), &mut Broken)
            .await
            .is_err()
    );
    assert_eq!(fs::read(dir.path().join("name.txt")).unwrap(), b"uploaded");
    for bad in ["../escape", "bad/name", ".", "bad\\name"] {
        assert!(
            service
                .upload(&checkout, &json!({"filename":bad}), &mut &b"bad"[..])
                .await
                .is_err()
        );
    }
}
#[cfg(unix)]
#[tokio::test]
async fn symlinks_escape_checks_and_absolute_preview_contract() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("secret"), "outside").unwrap();
    symlink(outside.path(), dir.path().join("escape")).unwrap();
    symlink(outside.path().join("secret"), dir.path().join("link")).unwrap();
    let checkout = checkout(dir.path());
    let service = WorkspaceService::default();
    for (method, path) in [
        ("file.list", "escape"),
        ("file.read", "link"),
        ("file.delete", "escape/secret"),
    ] {
        assert!(
            service
                .dispatch(&checkout, method, &json!({"path":path}))
                .await
                .is_err()
        );
    }
    assert!(
        service
            .upload(&checkout, &json!({"filename":"link"}), &mut &b"bad"[..])
            .await
            .is_err()
    );
    let absolute = outside.path().join("secret");
    assert_eq!(
        call(&service, &checkout, "file.read", json!({"path":absolute})).await["text"],
        "outside"
    );
    let resolved = call(
        &service,
        &checkout,
        "file.resolve",
        json!({"paths":["link",absolute,"../outside","missing"]}),
    )
    .await;
    assert_eq!(resolved["files"].as_array().unwrap().len(), 1);
    call(&service, &checkout, "file.delete", json!({"path":"link"})).await;
    assert!(absolute.exists());
    for root in [".", "./.", "/"] {
        assert!(
            service
                .dispatch(&checkout, "file.delete", &json!({"path":root}))
                .await
                .is_err()
        );
    }
    git(dir.path(), &["init", "-b", "main"]);
    symlink(outside.path(), dir.path().join("dir-link")).unwrap();
    call(
        &service,
        &checkout,
        "git.file_action",
        json!({"action":"delete_untracked","path":"dir-link"}),
    )
    .await;
    assert!(outside.path().is_dir());
    fs::create_dir(dir.path().join("scratch")).unwrap();
    fs::write(dir.path().join("scratch/note"), "keep").unwrap();
    assert!(
        service
            .dispatch(
                &checkout,
                "git.file_action",
                &json!({"action":"delete_untracked","path":"scratch"})
            )
            .await
            .is_err()
    );
    assert!(dir.path().join("scratch/note").exists());
}
#[tokio::test]
async fn pull_fast_forwards_and_status_reports_tracking() {
    let origin = repository();
    let clone = tempfile::tempdir().unwrap();
    git(
        clone.path(),
        &["clone", origin.path().to_str().unwrap(), "."],
    );
    fs::write(origin.path().join("tracked.txt"), "remote\n").unwrap();
    git(origin.path(), &["commit", "-am", "remote"]);
    let checkout = checkout(clone.path());
    let service = WorkspaceService::default();
    call(&service, &checkout, "git.pull", json!({})).await;
    assert_eq!(
        fs::read_to_string(clone.path().join("tracked.txt")).unwrap(),
        "remote\n"
    );
    let status = call(&service, &checkout, "git.status", json!({})).await;
    assert_eq!(status["branch"], "main");
    assert_eq!(status["upstream"], "origin/main");
    assert_eq!(status["dirty"], false);
}

fn git(root: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn checkout(root: &std::path::Path) -> Checkout {
    Checkout {
        workspace_id: "w1".into(),
        path: root.to_str().unwrap().into(),
        repo_name: "Repo".into(),
    }
}

#[tokio::test]
async fn files_and_git_dispatch_real_checkout() {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-b", "main"]);
    fs::write(dir.path().join("new.txt"), "fresh\n").unwrap();
    let service = WorkspaceService::default();
    let checkout = checkout(dir.path());
    let list = service
        .dispatch(&checkout, "file.list", &json!({}))
        .await
        .unwrap();
    assert_eq!(list["workspace_id"], "w1");
    assert_eq!(list["entries"][0]["path"], "new.txt");
    let preview = service
        .dispatch(&checkout, "file.read", &json!({"path":"new.txt"}))
        .await
        .unwrap();
    assert_eq!(preview["text"], "fresh\n");
    assert!(
        service
            .dispatch(&checkout, "file.read", &json!({"path":"../outside"}))
            .await
            .is_err()
    );
    service
        .dispatch(
            &checkout,
            "git.file_action",
            &json!({"action":"stage", "path":"new.txt"}),
        )
        .await
        .unwrap();
    assert!(git(dir.path(), &["status", "--porcelain"]).contains("A  new.txt"));
}
