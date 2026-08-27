//! Connects to `lanclipd` over local IPC, auto-starting it if it isn't already running
//! (spec §11.1). This crate never opens a network endpoint of its own -- it only ever talks
//! to its own daemon over the platform IPC transport from `lcp-ipc`.

use std::time::Duration;

use lcp_ipc::client::IpcClient;

const AUTOSTART_RETRY_ATTEMPTS: u32 = 30;
const AUTOSTART_RETRY_DELAY: Duration = Duration::from_millis(150);
const IPC_CONNECT_TIMEOUT: Duration = Duration::from_millis(500);

pub async fn connect_or_autostart() -> anyhow::Result<IpcClient> {
    if let Ok(client) = try_connect().await {
        return Ok(client);
    }
    spawn_daemon()?;
    for _ in 0..AUTOSTART_RETRY_ATTEMPTS {
        tokio::time::sleep(AUTOSTART_RETRY_DELAY).await;
        if let Ok(client) = try_connect().await {
            return Ok(client);
        }
    }
    anyhow::bail!("started lanclipd but it never became reachable over IPC")
}

pub async fn try_connect() -> std::io::Result<IpcClient> {
    #[cfg(unix)]
    {
        let path = lcp_ipc::unix::default_socket_path()?;
        let stream = tokio::time::timeout(IPC_CONNECT_TIMEOUT, lcp_ipc::unix::connect(&path))
            .await
            .map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::TimedOut, "IPC connect timed out")
            })??;
        Ok(IpcClient::spawn(stream))
    }
    #[cfg(windows)]
    {
        let user_id = lcp_ipc::windows::current_user_identifier();
        let stream =
            tokio::time::timeout(IPC_CONNECT_TIMEOUT, lcp_ipc::windows::connect(&user_id))
                .await
                .map_err(|_| {
                    std::io::Error::new(std::io::ErrorKind::TimedOut, "IPC connect timed out")
                })??;
        Ok(IpcClient::spawn(stream))
    }
}

/// Waits until the daemon is no longer reachable, up to a generous timeout. Used by `daemon
/// restart` to avoid racing a new instance's bind against the old instance's IPC endpoint
/// still closing (spec §11.12).
pub async fn wait_until_unreachable(timeout: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if try_connect().await.is_err() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

pub fn spawn_daemon() -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    let daemon_name = if cfg!(windows) {
        "lanclipd.exe"
    } else {
        "lanclipd"
    };
    let daemon_path = exe.with_file_name(daemon_name);
    // The daemon logs to its own file (spec §6.4) and must outlive this CLI invocation, so it
    // is not attached to this terminal -- otherwise its own startup prints would interleave
    // with ours, and writes would start failing once we exit and the console handle goes away.
    std::process::Command::new(&daemon_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| anyhow::anyhow!("failed to start {}: {e}", daemon_path.display()))?;
    Ok(())
}
