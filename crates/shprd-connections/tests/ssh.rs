use shprd_connections::{Profile, RetryPolicy, SshTunnel, classify_ssh_failure, ssh_tunnel_argv};
#[test]
fn strict_argv_and_nonretryable_failures() {
    let args = ssh_tunnel_argv("user@host", &[("/tmp/local.sock", "/tmp/remote.sock")]).unwrap();
    assert!(
        args.windows(2)
            .any(|a| a == ["-o", "StrictHostKeyChecking=yes"])
    );
    assert_eq!(&args[args.len() - 2..], &["--", "user@host"]);
    assert!(ssh_tunnel_argv("-oProxyCommand=x", &[]).is_err());
    assert!(!classify_ssh_failure(255, "Permission denied (publickey)").retryable);
    assert!(!classify_ssh_failure(255, "Host key verification failed").retryable);
    assert!(classify_ssh_failure(255, "Connection refused").retryable);
}
#[test]
fn retry_budget_jitter_stability_and_cancelled_tickets() {
    let mut policy = RetryPolicy::default();
    policy.enable();
    let first = policy.schedule(true, 0.0).unwrap();
    assert_eq!(first.delay.as_millis(), 500);
    policy.disable();
    assert!(!policy.is_current(&first));
    assert!(policy.schedule(true, 1.0).is_none());
    policy.enable();
    for _ in 0..6 {
        assert!(policy.schedule(true, 1.0).is_some());
    }
    assert!(policy.schedule(true, 1.0).is_none());
    policy.stable(std::time::Duration::from_secs(30));
    assert!(policy.schedule(true, 1.0).is_some());
    assert!(policy.schedule(false, 0.5).is_none());
}
#[cfg(unix)]
#[tokio::test]
async fn isolated_subprocess_and_socket_surface_is_reaped_and_removed() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let executable = dir.path().join("ssh-fixture");
    let fifo = dir.path().join("input");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    std::fs::write(
        &executable,
        format!("#!/bin/sh\nexec /bin/cat '{}'\n", fifo.display()),
    )
    .unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let profile = Profile::parse(r#"{"id":"fixture","label":"Fixture","type":"ssh","ssh_destination":"unused-fixture-host","remote_control_socket_path":"/tmp/control","remote_client_socket_path":"/tmp/render","auto_connect":false}"#).unwrap();
    let mut tunnel = SshTunnel::spawn(&profile, &executable).await.unwrap();
    let paths = tunnel.paths().clone();
    let control = tokio::net::UnixListener::bind(&paths.control).unwrap();
    let render = tokio::net::UnixListener::bind(&paths.render).unwrap();
    tunnel.wait_ready().await.unwrap();
    let (stream, _) = control.accept().await.unwrap();
    drop(stream);
    let (stream, _) = render.accept().await.unwrap();
    drop(stream);
    let pid = tunnel.pid().unwrap();
    let owned = paths.control.parent().unwrap().to_owned();
    tunnel.stop().await.unwrap();
    assert!(!owned.exists());
    assert!(
        rustix::process::test_kill_process(
            rustix::process::Pid::from_raw(pid.try_into().unwrap()).unwrap()
        )
        .is_err()
    );
}
