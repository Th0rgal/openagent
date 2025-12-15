//! SSH helpers for the dashboard console + file explorer.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use uuid::Uuid;

use crate::config::ConsoleSshConfig;

/// A temporary SSH key file (best-effort cleanup on drop).
pub struct TempKeyFile {
    path: PathBuf,
}

impl TempKeyFile {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempKeyFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub async fn materialize_private_key(private_key: &str) -> anyhow::Result<TempKeyFile> {
    let name = format!("open_agent_console_key_{}.key", Uuid::new_v4());
    let path = std::env::temp_dir().join(name);
    let mut f = tokio::fs::File::create(&path).await?;
    f.write_all(private_key.as_bytes()).await?;
    f.flush().await?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perm = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(&path, perm)?;
    }

    Ok(TempKeyFile { path })
}

fn ssh_base_args(cfg: &ConsoleSshConfig, key_path: &Path) -> Vec<String> {
    vec![
        "-i".to_string(),
        key_path.to_string_lossy().to_string(),
        "-p".to_string(),
        cfg.port.to_string(),
        "-o".to_string(),
        "BatchMode=yes".to_string(),
        "-o".to_string(),
        "LogLevel=ERROR".to_string(),
        "-o".to_string(),
        "StrictHostKeyChecking=accept-new".to_string(),
        "-o".to_string(),
        // Keep known_hosts separate from system to avoid permission issues.
        format!(
            "UserKnownHostsFile={}",
            std::env::temp_dir()
                .join("open_agent_known_hosts")
                .to_string_lossy()
        ),
    ]
}

pub async fn ssh_exec(cfg: &ConsoleSshConfig, key_path: &Path, remote_cmd: &str, args: &[String]) -> anyhow::Result<String> {
    let mut cmd = Command::new("ssh");
    for a in ssh_base_args(cfg, key_path) {
        cmd.arg(a);
    }

    cmd.arg(format!("{}@{}", cfg.user, cfg.host));
    cmd.arg("--");
    cmd.arg(remote_cmd);
    for a in args {
        cmd.arg(a);
    }

    let out = tokio::time::timeout(Duration::from_secs(30), cmd.output()).await??;
    if !out.status.success() {
        return Err(anyhow::anyhow!(
            "ssh failed (code {:?}): {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Execute a remote command and feed `stdin_data` to its stdin (useful to avoid shell quoting issues).
pub async fn ssh_exec_with_stdin(
    cfg: &ConsoleSshConfig,
    key_path: &Path,
    remote_cmd: &str,
    args: &[String],
    stdin_data: &str,
) -> anyhow::Result<String> {
    let mut cmd = Command::new("ssh");
    for a in ssh_base_args(cfg, key_path) {
        cmd.arg(a);
    }

    cmd.arg(format!("{}@{}", cfg.user, cfg.host));
    cmd.arg("--");
    cmd.arg(remote_cmd);
    for a in args {
        cmd.arg(a);
    }

    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(stdin_data.as_bytes()).await?;
    }

    let out = tokio::time::timeout(Duration::from_secs(30), child.wait_with_output()).await??;
    if !out.status.success() {
        return Err(anyhow::anyhow!(
            "ssh failed (code {:?}): {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

pub async fn sftp_batch(cfg: &ConsoleSshConfig, key_path: &Path, batch: &str) -> anyhow::Result<()> {
    let mut cmd = Command::new("sftp");
    cmd.arg("-b").arg("-");
    cmd.arg("-i").arg(key_path);
    cmd.arg("-P").arg(cfg.port.to_string());
    cmd.arg("-o").arg("BatchMode=yes");
    cmd.arg("-o").arg("LogLevel=ERROR");
    cmd.arg("-o").arg("StrictHostKeyChecking=accept-new");
    cmd.arg("-o").arg(format!(
        "UserKnownHostsFile={}",
        std::env::temp_dir()
            .join("open_agent_known_hosts")
            .to_string_lossy()
    ));
    cmd.arg(format!("{}@{}", cfg.user, cfg.host));

    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(batch.as_bytes()).await?;
    }
    let out = tokio::time::timeout(Duration::from_secs(120), child.wait_with_output()).await??;
    if !out.status.success() {
        return Err(anyhow::anyhow!(
            "sftp failed (code {:?}): {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}



