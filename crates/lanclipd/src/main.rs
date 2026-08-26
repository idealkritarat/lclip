use std::sync::Arc;

use lcp_core::config::Config;
use lcp_core::state::{AppState, SharedState};
use lcp_ipc::{EventSender, RequestHandler};
use lcp_protocol::ipc::{error_codes, methods, IpcRequest, IpcResponse};
use lcp_protocol::{IPC_PROTOCOL_VERSION, NETWORK_PROTOCOL_VERSION};
use tokio::sync::{Notify, RwLock};

struct Dispatcher {
    state: SharedState,
    shutdown: Arc<Notify>,
}

impl RequestHandler for Dispatcher {
    async fn handle(&self, request: IpcRequest, _events: EventSender) -> IpcResponse {
        if request.ipc_version != IPC_PROTOCOL_VERSION {
            return IpcResponse::err(
                IPC_PROTOCOL_VERSION,
                request.id,
                error_codes::VERSION_MISMATCH,
                format!(
                    "daemon speaks ipc version {IPC_PROTOCOL_VERSION}, client sent {}",
                    request.ipc_version
                ),
            );
        }

        match request.method.as_str() {
            methods::HELLO => IpcResponse::ok(
                IPC_PROTOCOL_VERSION,
                request.id,
                serde_json::json!({
                    "daemon_version": env!("CARGO_PKG_VERSION"),
                    "ipc_protocol_version": IPC_PROTOCOL_VERSION,
                    "network_protocol_version": NETWORK_PROTOCOL_VERSION,
                }),
            ),
            methods::GET_STATUS => {
                let state = self.state.read().await;
                IpcResponse::ok(
                    IPC_PROTOCOL_VERSION,
                    request.id,
                    serde_json::json!({
                        "version": env!("CARGO_PKG_VERSION"),
                        "ipc_protocol_version": IPC_PROTOCOL_VERSION,
                        "network_protocol_version": NETWORK_PROTOCOL_VERSION,
                        "uptime_secs": state.uptime().as_secs(),
                        "endpoint_id_prefix": short_id(&state.identity.endpoint_id().to_string()),
                        "relay_mode": state.config.network.relay_mode,
                        "trusted_peer_count": state.config.trusted_peers.len(),
                        "online_peer_count": 0,
                        "history_memory_only": true,
                        "autostart": state.config.daemon.autostart,
                    }),
                )
            }
            methods::GET_CONFIG => {
                let state = self.state.read().await;
                match request.params.get("key").and_then(|v| v.as_str()) {
                    Some(key) => match state.config.get_by_key(key) {
                        Some(value) => IpcResponse::ok(IPC_PROTOCOL_VERSION, request.id, value),
                        None => IpcResponse::err(
                            IPC_PROTOCOL_VERSION,
                            request.id,
                            error_codes::INVALID_PARAMS,
                            format!("unknown config key {key:?}"),
                        ),
                    },
                    None => {
                        let value =
                            serde_json::to_value(&state.config).unwrap_or(serde_json::Value::Null);
                        IpcResponse::ok(IPC_PROTOCOL_VERSION, request.id, value)
                    }
                }
            }
            methods::SET_CONFIG => {
                let (key, value) = match (
                    request.params.get("key").and_then(|v| v.as_str()),
                    request.params.get("value").and_then(|v| v.as_str()),
                ) {
                    (Some(k), Some(v)) => (k.to_string(), v.to_string()),
                    _ => {
                        return IpcResponse::err(
                            IPC_PROTOCOL_VERSION,
                            request.id,
                            error_codes::INVALID_PARAMS,
                            "expected string 'key' and 'value'",
                        )
                    }
                };
                let mut state = self.state.write().await;
                match state.config.set_by_key(&key, &value) {
                    Ok(()) => match state.config.save() {
                        Ok(()) => IpcResponse::ok(
                            IPC_PROTOCOL_VERSION,
                            request.id,
                            serde_json::json!({"key": key, "value": value}),
                        ),
                        Err(e) => IpcResponse::err(
                            IPC_PROTOCOL_VERSION,
                            request.id,
                            error_codes::INTERNAL,
                            e.to_string(),
                        ),
                    },
                    Err(e) => IpcResponse::err(
                        IPC_PROTOCOL_VERSION,
                        request.id,
                        error_codes::INVALID_PARAMS,
                        e.to_string(),
                    ),
                }
            }
            methods::LIST_PEERS => {
                let state = self.state.read().await;
                let peers: Vec<_> = state
                    .config
                    .trusted_peers
                    .iter()
                    .map(|p| {
                        serde_json::json!({
                            "endpoint_id": p.endpoint_id,
                            "alias": p.alias,
                            "device_name": p.device_name,
                            "status": "offline",
                            "path": "-",
                        })
                    })
                    .collect();
                IpcResponse::ok(
                    IPC_PROTOCOL_VERSION,
                    request.id,
                    serde_json::Value::Array(peers),
                )
            }
            methods::SHUTDOWN => {
                self.shutdown.notify_one();
                IpcResponse::ok(
                    IPC_PROTOCOL_VERSION,
                    request.id,
                    serde_json::json!({"shutting_down": true}),
                )
            }
            other => IpcResponse::err(
                IPC_PROTOCOL_VERSION,
                request.id,
                error_codes::UNKNOWN_METHOD,
                format!("unknown method {other:?}"),
            ),
        }
    }
}

fn short_id(id: &str) -> String {
    id.chars().take(12).collect()
}

fn init_logging() -> anyhow::Result<tracing_appender::non_blocking::WorkerGuard> {
    let logs_dir = lcp_core::config::logs_dir()?;
    std::fs::create_dir_all(&logs_dir)?;
    let file_appender = tracing_appender::rolling::daily(&logs_dir, "lanclipd.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_env_filter(filter)
        .with_ansi(false)
        .init();
    Ok(guard)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _log_guard = init_logging()?;

    let config = Config::load_or_create()?;
    let identity = lcp_core::identity::load_or_create(!config.trusted_peers.is_empty())?;
    tracing::info!(
        endpoint_id = %short_id(&identity.endpoint_id().to_string()),
        "identity loaded"
    );

    let state: SharedState = Arc::new(RwLock::new(AppState::new(identity, config)));
    let shutdown = Arc::new(Notify::new());
    let handler = Arc::new(Dispatcher {
        state: state.clone(),
        shutdown: shutdown.clone(),
    });

    #[cfg(unix)]
    let socket_path = {
        let socket_path = lcp_ipc::unix::default_socket_path()?;
        let listener = match lcp_ipc::unix::bind(&socket_path).await {
            Ok(listener) => listener,
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                println!("lanclipd is already running.");
                return Ok(());
            }
            Err(e) => return Err(e.into()),
        };
        tracing::info!(path = %socket_path.display(), "listening on unix socket");
        tokio::spawn(lcp_ipc::unix::serve(listener, handler));
        socket_path
    };

    #[cfg(windows)]
    {
        let (listener, addr) = match lcp_ipc::windows::bind_first_instance() {
            Ok(pair) => pair,
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                println!("lanclipd is already running.");
                return Ok(());
            }
            Err(e) => return Err(e.into()),
        };
        tracing::info!(pipe = %addr, "listening on named pipe");
        tokio::spawn(lcp_ipc::windows::serve(listener, addr, handler));
    }

    println!("lanclipd started.");

    tokio::select! {
        _ = shutdown.notified() => {
            tracing::info!("shutdown requested over IPC");
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("shutdown requested via ctrl-c");
        }
    }

    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(&socket_path);
    }

    Ok(())
}
