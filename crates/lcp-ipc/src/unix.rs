//! Unix domain socket transport (macOS). Not compiled or tested on this Windows development
//! machine -- verify on real macOS/CI before relying on it.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;

use tokio::net::{UnixListener, UnixStream};

use crate::server::{handle_connection, RequestHandler};

/// `~/Library/Application Support/lcp/lanclipd.sock`. Duplicated in spirit from
/// `lcp-core::config::app_dir` rather than shared, since `lcp-cli` must not depend on
/// `lcp-core` (spec §4.1's dependency direction) but still needs to find this path.
pub fn default_socket_path() -> std::io::Result<std::path::PathBuf> {
    let base = directories::BaseDirs::new()
        .ok_or_else(|| std::io::Error::other("could not determine platform config directory"))?;
    Ok(base.config_dir().join("lcp").join("lanclipd.sock"))
}

/// Binds the socket, removing a stale one left behind by a daemon that is no longer running.
/// A socket is considered stale only after an actual connect attempt fails -- never removed
/// just because the path exists, since a live daemon might own it.
pub async fn bind(socket_path: &Path) -> std::io::Result<UnixListener> {
    if socket_path.exists() {
        match UnixStream::connect(socket_path).await {
            Ok(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AddrInUse,
                    "a daemon is already listening on this socket",
                ));
            }
            Err(_) => {
                std::fs::remove_file(socket_path)?;
            }
        }
    }
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let listener = UnixListener::bind(socket_path)?;
    std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

pub async fn serve<H: RequestHandler>(listener: UnixListener, handler: Arc<H>) {
    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                let handler = handler.clone();
                tokio::spawn(async move {
                    handle_connection(stream, handler).await;
                });
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to accept IPC connection");
            }
        }
    }
}

pub async fn connect(socket_path: &Path) -> std::io::Result<UnixStream> {
    UnixStream::connect(socket_path).await
}
