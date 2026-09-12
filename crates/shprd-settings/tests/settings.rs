#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use serde_json::{Value, json};
use shprd_settings::{
    DEFAULT_WORKSPACE_AUTO_SYNC_INTERVAL_MINUTES, Error, GuiSettings, RepoSettingsPatch,
    SettingsIdentity, SettingsService, WorkspaceMetadata, connection_settings_prefix,
    repo_settings_key, workspace_auto_sync_settings_key, workspace_repo_settings_key,
};
use std::{fs, path::PathBuf, sync::Arc};

fn identity(connection_id: Option<&str>, host: Option<&str>) -> SettingsIdentity {
    SettingsIdentity {
        connection_id: connection_id.map(str::to_owned),
        host: host.map(str::to_owned),
    }
}
#[tokio::test]
async fn separate_service_instances_share_mutation_queue() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let a = SettingsService::new(&path, identity(Some("alpha"), None)).unwrap();
    let b = SettingsService::new(&path, identity(Some("alpha"), None)).unwrap();
    let first = a.update_repo(
        "connection:alpha:local:first",
        RepoSettingsPatch {
            worktree_hooks_enabled: Some(false),
            custom: None,
        },
    );
    let second = b.update_repo(
        "connection:alpha:local:second",
        RepoSettingsPatch {
            worktree_hooks_enabled: Some(true),
            custom: None,
        },
    );
    let (_, _) = tokio::join!(first, second);
    let settings = a.read().await.unwrap();
    assert_eq!(settings.repositories.len(), 2);
}

#[tokio::test]
async fn auto_sync_result_updates_existing_entry_only() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let service = SettingsService::new(&path, identity(Some("alpha"), None)).unwrap();
    service
        .update_workspace_auto_sync(&workspace(), true, false)
        .await
        .unwrap();
    assert!(
        service
            .record_auto_sync_result(
                "connection:alpha:local:/checkout",
                shprd_settings::AutoSyncResult {
                    checkout_path: "/checkout".into(),
                    host: None,
                    status: shprd_settings::WorkspaceAutoSyncStatus::Updated,
                    message: Some("ok".into()),
                    branch: Some("main".into()),
                    last_run_at: "2026-09-12T00:00:00Z".into(),
                }
            )
            .await
            .unwrap()
    );
    assert!(
        !service
            .record_auto_sync_result(
                "connection:alpha:local:missing",
                shprd_settings::AutoSyncResult {
                    checkout_path: "/missing".into(),
                    host: None,
                    status: shprd_settings::WorkspaceAutoSyncStatus::Failed,
                    message: None,
                    branch: None,
                    last_run_at: "now".into(),
                }
            )
            .await
            .unwrap()
    );
    let entry =
        service.read().await.unwrap().workspace_auto_sync["connection:alpha:local:/checkout"]
            .clone();
    assert_eq!(
        entry.last_status,
        Some(shprd_settings::WorkspaceAutoSyncStatus::Updated)
    );
    assert_eq!(entry.last_message.as_deref(), Some("ok"));
}

fn workspace() -> WorkspaceMetadata {
    WorkspaceMetadata {
        workspace_id: "w1".into(),
        label: Some("Repo".into()),
        repo_key: Some("same-repo".into()),
        repo_root: Some("/repo".into()),
        checkout_path: Some("/checkout".into()),
    }
}

#[tokio::test]
async fn real_temp_file_preserves_schema_keys_and_normalizes_values() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let service = SettingsService::new(&path, identity(None, None)).unwrap();
    assert_eq!(service.read().await.unwrap(), GuiSettings::default());
    assert!(!path.exists());
    fs::write(&path, json!({
        "version": 99,
        "repositories": {"local:repo": {"worktree_hooks_enabled": false, "custom": {"color": "blue"}}, "bad": "skip"},
        "workspace_auto_sync": {"local:/repo": {"enabled": true, "interval_minutes": 2.6, "last_status": "updated"}, "invalid": {"interval_minutes": 0, "last_status": "nope"}},
        "custom": {"future": {"key": true}}
    }).to_string()).unwrap();
    let settings = SettingsService::new(&path, identity(None, None))
        .unwrap()
        .read()
        .await
        .unwrap();
    assert_eq!(settings.version, 1);
    assert!(!settings.repositories.contains_key("bad"));
    assert_eq!(
        settings.repositories["local:repo"].worktree_hooks_enabled,
        Some(false)
    );
    assert_eq!(
        settings.workspace_auto_sync["local:/repo"].interval_minutes,
        3
    );
    assert_eq!(
        settings.workspace_auto_sync["invalid"].interval_minutes,
        DEFAULT_WORKSPACE_AUTO_SYNC_INTERVAL_MINUTES
    );
    assert_eq!(settings.custom["future"], json!({"key": true}));
}

#[tokio::test]
async fn corrupt_json_defaults_without_destroying_file_until_update() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    fs::write(&path, "{not-json").unwrap();
    let service = SettingsService::new(&path, identity(None, None)).unwrap();
    assert_eq!(service.read().await.unwrap(), GuiSettings::default());
    assert_eq!(fs::read_to_string(&path).unwrap(), "{not-json");
    service
        .update_repo(
            "local:repo",
            RepoSettingsPatch {
                worktree_hooks_enabled: Some(false),
                custom: None,
            },
        )
        .await
        .unwrap();
    let persisted: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        persisted["repositories"]["local:repo"]["worktree_hooks_enabled"],
        false
    );
}

#[tokio::test]
async fn keys_preserve_legacy_format_and_isolate_connection_host_namespaces() {
    let legacy = identity(Some("legacy-default"), None);
    let alpha = identity(Some("alpha:local:beta"), Some("dev-host"));
    assert_eq!(
        connection_settings_prefix(legacy.connection_id.as_deref()),
        ""
    );
    assert_eq!(
        repo_settings_key("same-repo", &legacy).unwrap(),
        "local:same-repo"
    );
    assert_eq!(
        repo_settings_key("same-repo", &alpha).unwrap(),
        "connection:alpha%3Alocal%3Abeta:ssh:dev-host:same-repo"
    );
    assert_eq!(
        workspace_repo_settings_key(&workspace(), &identity(Some("alpha"), None))
            .unwrap()
            .unwrap(),
        "connection:alpha:local:same-repo"
    );
    assert_eq!(
        workspace_auto_sync_settings_key(
            " /same/checkout ",
            &identity(Some("alpha"), Some("same-host"))
        )
        .unwrap(),
        "connection:alpha:ssh:same-host:/same/checkout"
    );
    assert!(workspace_auto_sync_settings_key(" ", &legacy).is_err());
}

#[tokio::test]
async fn ownership_rejects_cross_connection_reads_updates_and_lists() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let alpha = SettingsService::new(&path, identity(Some("alpha"), None)).unwrap();
    alpha
        .update_repo(
            "connection:alpha:local:repo",
            RepoSettingsPatch {
                worktree_hooks_enabled: Some(false),
                custom: None,
            },
        )
        .await
        .unwrap();
    alpha
        .update_workspace_auto_sync(&workspace(), true, false)
        .await
        .unwrap();
    let beta = SettingsService::new(&path, identity(Some("beta"), None)).unwrap();
    assert!(matches!(
        beta.update_repo("connection:alpha:local:repo", RepoSettingsPatch::default())
            .await,
        Err(Error::Ownership)
    ));
    assert!(matches!(
        beta.update_auto_sync_key("connection:alpha:local:/checkout", false)
            .await,
        Err(Error::Ownership)
    ));
    assert!(
        beta.list_workspace_auto_sync(&|_| false)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        !alpha
            .list_workspace_auto_sync(&|_| true)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        !alpha
            .repo_worktree_hooks_enabled(Some("connection:alpha:local:repo"))
            .await
            .unwrap()
    );
    assert!(matches!(
        alpha
            .repo_worktree_hooks_enabled(Some("connection:beta:local:repo"))
            .await,
        Err(Error::Ownership)
    ));
}

#[tokio::test]
async fn concurrent_updates_serialize_read_modify_write_without_lost_keys() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let service = Arc::new(SettingsService::new(&path, identity(Some("alpha"), None)).unwrap());
    let mut tasks = Vec::new();
    for index in 0..24 {
        let service = Arc::clone(&service);
        tasks.push(tokio::spawn(async move {
            service
                .update_repo(
                    &format!("connection:alpha:local:repo-{index}"),
                    RepoSettingsPatch {
                        worktree_hooks_enabled: Some(index % 2 == 0),
                        custom: None,
                    },
                )
                .await
        }));
    }
    for task in tasks {
        task.await.unwrap().unwrap();
    }
    let settings = service.read().await.unwrap();
    assert_eq!(settings.repositories.len(), 24);
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.ends_with('\n'));
    assert!(serde_json::from_str::<Value>(&text).is_ok());
}

#[tokio::test]
async fn atomic_write_leaves_no_temp_files_and_preserves_permissions() {
    let dir = tempfile::tempdir().unwrap();
    let path = PathBuf::from(dir.path()).join("settings.json");
    let service = SettingsService::new(&path, identity(None, None)).unwrap();
    service
        .update_repo(
            "local:repo",
            RepoSettingsPatch {
                worktree_hooks_enabled: Some(false),
                custom: Some(serde_json::Map::from_iter([("x".into(), json!(1))])),
            },
        )
        .await
        .unwrap();
    assert!(
        dir.path()
            .read_dir()
            .unwrap()
            .all(|entry| entry.unwrap().file_name() == "settings.json")
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        service
            .update_repo(
                "local:repo",
                RepoSettingsPatch {
                    worktree_hooks_enabled: Some(true),
                    custom: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn settings_symlink_is_refused_for_read_and_write() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real.json");
    let link = dir.path().join("settings.json");
    fs::write(&real, "{}").unwrap();
    symlink(&real, &link).unwrap();
    assert!(matches!(
        SettingsService::new(&link, identity(None, None)),
        Err(Error::Symlink)
    ));
}
