use crate::{
    Error, Result,
    error::invalid,
    paths::validate_remote_path,
    ssh::{classify_ssh_failure, ssh_command_argv},
    tunnel::bounded_tail,
};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{io::AsyncReadExt, process::Command, time::timeout};
pub(crate) async fn remote_home(executable: &Path, host: &str) -> Result<String> {
    let args = ssh_command_argv(host, "printf %s \"$HOME\"")?;
    let mut child = Command::new(executable)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| invalid("missing SSH stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| invalid("missing SSH stderr"))?;
    let read = async {
        let mut bytes = Vec::new();
        stdout.take(4097).read_to_end(&mut bytes).await?;
        if bytes.len() > 4096 {
            return Err(invalid("remote HOME is too long"));
        }
        Ok::<_, Error>(bytes)
    };
    let run = async {
        let (stdout, stderr, status) = tokio::try_join!(
            read,
            async { bounded_tail(stderr, 16384).await.map_err(Error::from) },
            async { child.wait().await.map_err(Error::from) }
        )?;
        Ok::<_, Error>((stdout, stderr, status))
    };
    let result = timeout(Duration::from_secs(10), run)
        .await
        .map_err(|_| invalid("remote HOME probe timed out"));
    let (stdout, stderr, status) = match result {
        Ok(Ok(output)) => output,
        Ok(Err(e)) | Err(e) => {
            child.kill().await?;
            return Err(e);
        }
    };
    if !status.success() {
        return Err(classify_ssh_failure(status.code().unwrap_or(-1), &stderr).into());
    }
    let home = String::from_utf8(stdout)
        .map_err(|_| invalid("invalid remote HOME"))?
        .trim()
        .to_owned();
    validate_remote_path(&home)?;
    if home.is_empty() {
        return Err(invalid("could not resolve remote HOME"));
    }
    Ok(home)
}
