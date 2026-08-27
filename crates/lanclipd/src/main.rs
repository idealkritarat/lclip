use std::sync::Arc;
use std::time::Duration;

use iroh_tickets::Ticket;
use uuid::Uuid;

use lcp_core::config::Config;
use lcp_core::conversation::MessageStatus;
use lcp_core::pairing;
use lcp_core::peers::{self, Resolved};
use lcp_core::state::{AppState, SharedState};
use lcp_core::transport;
use lcp_ipc::{EventSender, RequestHandler};
use lcp_protocol::ipc::{error_codes, methods, IpcRequest, IpcResponse};
use lcp_protocol::network::validate_text_len;
use lcp_protocol::ticket::{clamp_invite_ttl_secs, PairingTicketV1, DEFAULT_INVITE_TTL_SECS};
use lcp_protocol::{IPC_PROTOCOL_VERSION, NETWORK_PROTOCOL_VERSION};
use tokio::sync::{Notify, RwLock};

/// Resolves a peer identifier against the trusted-peer list, or returns the error code/message
/// to report (not-found vs. ambiguous both map to CLI exit code 3, per spec §11.13).
fn resolve_peer<'a>(
    trusted_peers: &'a [lcp_core::peers::TrustedPeer],
    identifier: &str,
) -> Result<&'a lcp_core::peers::TrustedPeer, (&'static str, String)> {
    match peers::resolve_identifier(trusted_peers, identifier) {
        Resolved::Found(peer) => Ok(peer),
        Resolved::NotFound => Err((
            error_codes::PEER_NOT_FOUND,
            format!("no trusted peer matches {identifier:?}"),
        )),
        Resolved::Ambiguous(matches) => {
            let candidates: Vec<&str> = matches.iter().map(|p| p.alias.as_str()).collect();
            Err((
                error_codes::PEER_AMBIGUOUS,
                format!(
                    "{identifier:?} matches more than one peer: {}",
                    candidates.join(", ")
                ),
            ))
        }
    }
}

fn message_json(message: &lcp_core::conversation::StoredMessage) -> serde_json::Value {
    serde_json::json!({
        "message_id": message.id,
        "peer_id": message.peer_id,
        "direction": message.direction,
        "sender_label": message.sender_label,
        "text": message.text,
        "sent_at_unix_ms": message.sent_at_unix_ms,
        "received_at_unix_ms": message.received_at_unix_ms,
        "receive_sequence": message.receive_sequence,
        "status": message.status,
    })
}

fn now_unix_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

struct Dispatcher {
    state: SharedState,
    endpoint: iroh::Endpoint,
    shutdown: Arc<Notify>,
}

impl Dispatcher {
    async fn resolve_pairing_decision(&self, request: &IpcRequest, confirmed: bool) -> IpcResponse {
        let pairing_id = match request
            .params
            .get("pairing_id")
            .and_then(|v| v.as_str())
            .and_then(|s| Uuid::parse_str(s).ok())
        {
            Some(id) => id,
            None => {
                return IpcResponse::err(
                    IPC_PROTOCOL_VERSION,
                    request.id,
                    error_codes::INVALID_PARAMS,
                    "expected a valid uuid string 'pairing_id'",
                )
            }
        };
        let sender = {
            let mut state = self.state.write().await;
            state
                .pending_pairings
                .get_mut(&pairing_id)
                .and_then(|p| p.local_decision_tx.take())
        };
        match sender {
            Some(tx) => {
                let _ = tx.send(confirmed);
                IpcResponse::ok(IPC_PROTOCOL_VERSION, request.id, serde_json::json!({}))
            }
            None => IpcResponse::err(
                IPC_PROTOCOL_VERSION,
                request.id,
                error_codes::PAIRING_FAILED,
                "no such pending pairing (already decided, rejected, or expired)",
            ),
        }
    }
}

impl RequestHandler for Dispatcher {
    async fn handle(&self, request: IpcRequest, events: EventSender) -> IpcResponse {
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
                let online_peer_count = state
                    .config
                    .trusted_peers
                    .iter()
                    .filter(|p| {
                        state.peer_connections.status_of(&p.endpoint_id).0
                            == lcp_core::connection::PeerStatus::Online
                    })
                    .count();
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
                        "online_peer_count": online_peer_count,
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
                        Ok(()) => {
                            if key == "history.limit_per_peer" {
                                let limit = state.config.history.limit_per_peer as usize;
                                state.conversations.set_history_limit(limit);
                            }
                            IpcResponse::ok(
                                IPC_PROTOCOL_VERSION,
                                request.id,
                                serde_json::json!({"key": key, "value": value}),
                            )
                        }
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
            methods::RESET_CONFIG => {
                let mut state = self.state.write().await;
                let trusted_peers = state.config.trusted_peers.clone();
                state.config = Config {
                    trusted_peers,
                    ..Config::default()
                };
                let history_limit = state.config.history.limit_per_peer as usize;
                state.conversations.set_history_limit(history_limit);
                match state.config.save() {
                    Ok(()) => {
                        let value =
                            serde_json::to_value(&state.config).unwrap_or(serde_json::Value::Null);
                        IpcResponse::ok(IPC_PROTOCOL_VERSION, request.id, value)
                    }
                    Err(e) => IpcResponse::err(
                        IPC_PROTOCOL_VERSION,
                        request.id,
                        error_codes::INTERNAL,
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
                        let (status, path) = state.peer_connections.status_of(&p.endpoint_id);
                        serde_json::json!({
                            "endpoint_id": p.endpoint_id,
                            "alias": p.alias,
                            "device_name": p.device_name,
                            "status": status,
                            "path": path,
                        })
                    })
                    .collect();
                IpcResponse::ok(
                    IPC_PROTOCOL_VERSION,
                    request.id,
                    serde_json::Value::Array(peers),
                )
            }
            methods::RENAME_PEER => {
                let (peer_ident, alias) = match (
                    request.params.get("peer").and_then(|v| v.as_str()),
                    request.params.get("alias").and_then(|v| v.as_str()),
                ) {
                    (Some(p), Some(a)) => (p, a),
                    _ => {
                        return IpcResponse::err(
                            IPC_PROTOCOL_VERSION,
                            request.id,
                            error_codes::INVALID_PARAMS,
                            "expected string 'peer' and 'alias'",
                        )
                    }
                };
                let mut state = self.state.write().await;
                let endpoint_id = match resolve_peer(&state.config.trusted_peers, peer_ident) {
                    Ok(peer) => peer.endpoint_id.clone(),
                    Err((code, message)) => {
                        return IpcResponse::err(IPC_PROTOCOL_VERSION, request.id, code, message)
                    }
                };
                let alias = match peers::rename_trusted_peer_alias(
                    &mut state.config.trusted_peers,
                    &endpoint_id,
                    alias,
                ) {
                    Ok(alias) => alias,
                    Err(e) => {
                        return IpcResponse::err(
                            IPC_PROTOCOL_VERSION,
                            request.id,
                            error_codes::INVALID_PARAMS,
                            e.to_string(),
                        )
                    }
                };
                if let Err(e) = state.config.save() {
                    return IpcResponse::err(
                        IPC_PROTOCOL_VERSION,
                        request.id,
                        error_codes::INTERNAL,
                        e.to_string(),
                    );
                }
                IpcResponse::ok(
                    IPC_PROTOCOL_VERSION,
                    request.id,
                    serde_json::json!({"endpoint_id": endpoint_id, "alias": alias}),
                )
            }
            methods::CREATE_INVITE => {
                let ttl_secs = clamp_invite_ttl_secs(
                    request
                        .params
                        .get("ttl_secs")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(DEFAULT_INVITE_TTL_SECS),
                );

                let _ = tokio::time::timeout(Duration::from_secs(10), self.endpoint.online()).await;
                let endpoint_ticket =
                    iroh_tickets::endpoint::EndpointTicket::new(self.endpoint.addr());
                let expires_at_unix_ms = now_unix_ms() + (ttl_secs as i64) * 1000;
                let invite_secret = pairing::random_secret();

                let (display_name, device_name) = {
                    let state = self.state.read().await;
                    (
                        state.config.user.name.clone(),
                        state.config.user.device_name.clone(),
                    )
                };

                let pairing_ticket = PairingTicketV1::new(
                    endpoint_ticket.encode_string(),
                    invite_secret,
                    expires_at_unix_ms,
                    display_name,
                    device_name,
                );
                let ticket_text = match pairing_ticket.encode() {
                    Ok(t) => t,
                    Err(e) => {
                        return IpcResponse::err(
                            IPC_PROTOCOL_VERSION,
                            request.id,
                            error_codes::INTERNAL,
                            e.to_string(),
                        )
                    }
                };

                {
                    let mut state = self.state.write().await;
                    state.active_invites.push(pairing::ActiveInvite {
                        invite_secret,
                        created_at: std::time::Instant::now(),
                        expires_at: std::time::Instant::now() + Duration::from_secs(ttl_secs),
                        ticket_text: ticket_text.clone(),
                        attempts: 0,
                    });
                }

                IpcResponse::ok(
                    IPC_PROTOCOL_VERSION,
                    request.id,
                    serde_json::json!({"ticket": ticket_text, "ttl_secs": ttl_secs}),
                )
            }
            methods::CANCEL_INVITE => {
                let mut state = self.state.write().await;
                state.active_invites.clear();
                IpcResponse::ok(IPC_PROTOCOL_VERSION, request.id, serde_json::json!({}))
            }
            methods::JOIN_INVITE => {
                let ticket = match request.params.get("ticket").and_then(|v| v.as_str()) {
                    Some(t) => t,
                    None => {
                        return IpcResponse::err(
                            IPC_PROTOCOL_VERSION,
                            request.id,
                            error_codes::INVALID_PARAMS,
                            "expected string 'ticket'",
                        )
                    }
                };
                match pairing::join_pairing(&self.endpoint, &self.state, ticket).await {
                    Ok((pairing_id, verification_string)) => IpcResponse::ok(
                        IPC_PROTOCOL_VERSION,
                        request.id,
                        serde_json::json!({
                            "pairing_id": pairing_id,
                            "verification_string": verification_string,
                        }),
                    ),
                    Err(e) => IpcResponse::err(
                        IPC_PROTOCOL_VERSION,
                        request.id,
                        error_codes::PAIRING_FAILED,
                        e.to_string(),
                    ),
                }
            }
            methods::CONFIRM_PAIRING => self.resolve_pairing_decision(&request, true).await,
            methods::REJECT_PAIRING => self.resolve_pairing_decision(&request, false).await,
            methods::UNPAIR_PEER => {
                let peer_ident = match request.params.get("peer").and_then(|v| v.as_str()) {
                    Some(p) => p,
                    None => {
                        return IpcResponse::err(
                            IPC_PROTOCOL_VERSION,
                            request.id,
                            error_codes::INVALID_PARAMS,
                            "expected string 'peer'",
                        )
                    }
                };
                let mut state = self.state.write().await;
                let endpoint_id = match resolve_peer(&state.config.trusted_peers, peer_ident) {
                    Ok(peer) => peer.endpoint_id.clone(),
                    Err((code, message)) => {
                        return IpcResponse::err(IPC_PROTOCOL_VERSION, request.id, code, message)
                    }
                };
                // Revoke trust and close any active connection immediately (spec §7.8) --
                // future connection attempts from this peer will also fail authorization.
                state.peer_connections.close_and_forget(&endpoint_id);
                peers::remove_trusted_peer(&mut state.config.trusted_peers, &endpoint_id);
                if let Err(e) = state.config.save() {
                    return IpcResponse::err(
                        IPC_PROTOCOL_VERSION,
                        request.id,
                        error_codes::INTERNAL,
                        e.to_string(),
                    );
                }
                IpcResponse::ok(IPC_PROTOCOL_VERSION, request.id, serde_json::json!({}))
            }
            methods::SEND_TEXT => {
                let peer_ident = match request.params.get("peer").and_then(|v| v.as_str()) {
                    Some(p) => p,
                    None => {
                        return IpcResponse::err(
                            IPC_PROTOCOL_VERSION,
                            request.id,
                            error_codes::INVALID_PARAMS,
                            "expected string 'peer'",
                        )
                    }
                };
                let text = match request.params.get("text").and_then(|v| v.as_str()) {
                    Some(t) if !t.is_empty() => t,
                    Some(_) => {
                        return IpcResponse::err(
                            IPC_PROTOCOL_VERSION,
                            request.id,
                            error_codes::INVALID_PARAMS,
                            "text must not be empty",
                        )
                    }
                    None => {
                        return IpcResponse::err(
                            IPC_PROTOCOL_VERSION,
                            request.id,
                            error_codes::INVALID_PARAMS,
                            "expected string 'text'",
                        )
                    }
                };
                if let Err(e) = validate_text_len(text) {
                    return IpcResponse::err(
                        IPC_PROTOCOL_VERSION,
                        request.id,
                        error_codes::LIMIT_EXCEEDED,
                        e.to_string(),
                    );
                }

                let (endpoint_id, my_endpoint_id) = {
                    let state = self.state.read().await;
                    match resolve_peer(&state.config.trusted_peers, peer_ident) {
                        Ok(peer) => (
                            peer.endpoint_id.clone(),
                            state.identity.endpoint_id().to_string(),
                        ),
                        Err((code, message)) => {
                            return IpcResponse::err(
                                IPC_PROTOCOL_VERSION,
                                request.id,
                                code,
                                message,
                            )
                        }
                    }
                };
                let public_key: iroh::PublicKey = match endpoint_id.parse() {
                    Ok(k) => k,
                    Err(_) => {
                        return IpcResponse::err(
                            IPC_PROTOCOL_VERSION,
                            request.id,
                            error_codes::INTERNAL,
                            "stored endpoint id is corrupt",
                        )
                    }
                };

                let message_id = Uuid::new_v4();
                {
                    let mut state = self.state.write().await;
                    let message = state.conversations.record_outgoing(
                        &endpoint_id,
                        message_id,
                        text.to_string(),
                        MessageStatus::Sending,
                    );
                    state.broadcast_event(
                        lcp_protocol::ipc::events::MESSAGE_UPDATED,
                        serde_json::json!({"message": message_json(&message)}),
                    );
                }

                let result = transport::send_text(
                    &self.state,
                    &self.endpoint,
                    public_key.into(),
                    &endpoint_id,
                    &my_endpoint_id,
                    message_id,
                    text,
                )
                .await;

                let mut state = self.state.write().await;
                match result {
                    Ok(()) => {
                        state.conversations.update_outgoing_status(
                            &endpoint_id,
                            message_id,
                            MessageStatus::Sent,
                        );
                        let payload = state
                            .conversations
                            .message_for(&endpoint_id, message_id)
                            .map(|message| serde_json::json!({"message": message_json(message)}));
                        if let Some(payload) = payload {
                            state.broadcast_event(
                                lcp_protocol::ipc::events::MESSAGE_UPDATED,
                                payload,
                            );
                        }
                        IpcResponse::ok(
                            IPC_PROTOCOL_VERSION,
                            request.id,
                            serde_json::json!({"message_id": message_id}),
                        )
                    }
                    Err(e) => {
                        state.conversations.update_outgoing_status(
                            &endpoint_id,
                            message_id,
                            MessageStatus::Failed,
                        );
                        let payload = state
                            .conversations
                            .message_for(&endpoint_id, message_id)
                            .map(|message| serde_json::json!({"message": message_json(message)}));
                        if let Some(payload) = payload {
                            state.broadcast_event(
                                lcp_protocol::ipc::events::MESSAGE_UPDATED,
                                payload,
                            );
                        }
                        IpcResponse::err(
                            IPC_PROTOCOL_VERSION,
                            request.id,
                            error_codes::PEER_OFFLINE,
                            e.to_string(),
                        )
                    }
                }
            }
            methods::GET_LATEST_INCOMING => {
                let state = self.state.read().await;
                let message = match request.params.get("peer").and_then(|v| v.as_str()) {
                    Some(ident) => match resolve_peer(&state.config.trusted_peers, ident) {
                        Ok(peer) => state.conversations.latest_incoming_for(&peer.endpoint_id),
                        Err((code, message)) => {
                            return IpcResponse::err(
                                IPC_PROTOCOL_VERSION,
                                request.id,
                                code,
                                message,
                            )
                        }
                    },
                    None => state.conversations.latest_incoming_global(),
                };
                match message {
                    Some(m) => IpcResponse::ok(IPC_PROTOCOL_VERSION, request.id, message_json(m)),
                    None => IpcResponse::err(
                        IPC_PROTOCOL_VERSION,
                        request.id,
                        error_codes::NO_MESSAGE,
                        "No messages received since daemon start",
                    ),
                }
            }
            methods::LIST_MESSAGES => {
                let state = self.state.read().await;
                let incoming_only = request
                    .params
                    .get("incoming_only")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let messages: Vec<&lcp_core::conversation::StoredMessage> =
                    match request.params.get("peer").and_then(|v| v.as_str()) {
                        Some(ident) => match resolve_peer(&state.config.trusted_peers, ident) {
                            Ok(peer) => {
                                let mut msgs: Vec<_> = state
                                    .conversations
                                    .messages_for(&peer.endpoint_id)
                                    .collect();
                                msgs.sort_by_key(|m| std::cmp::Reverse(m.received_at_unix_ms));
                                msgs
                            }
                            Err((code, message)) => {
                                return IpcResponse::err(
                                    IPC_PROTOCOL_VERSION,
                                    request.id,
                                    code,
                                    message,
                                )
                            }
                        },
                        None => state.conversations.all_messages_newest_first(),
                    };
                let messages: Vec<serde_json::Value> = messages
                    .into_iter()
                    .filter(|m| {
                        !incoming_only
                            || matches!(m.direction, lcp_core::conversation::Direction::Incoming)
                    })
                    .map(message_json)
                    .collect();
                IpcResponse::ok(
                    IPC_PROTOCOL_VERSION,
                    request.id,
                    serde_json::Value::Array(messages),
                )
            }
            methods::SUBSCRIBE => {
                let mut state = self.state.write().await;
                state.add_subscriber(events);
                IpcResponse::ok(IPC_PROTOCOL_VERSION, request.id, serde_json::json!({}))
            }
            methods::RUN_DIAGNOSTICS => {
                let results = lcp_core::diagnostics::run_checks(&self.state, &self.endpoint).await;
                IpcResponse::ok(IPC_PROTOCOL_VERSION, request.id, serde_json::json!(results))
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
    let endpoint = transport::bind_endpoint(identity.secret_key().clone()).await?;
    tracing::info!(
        endpoint_id = %short_id(&identity.endpoint_id().to_string()),
        "identity loaded and endpoint bound"
    );

    let state: SharedState = Arc::new(RwLock::new(AppState::new(identity, config)));
    let shutdown = Arc::new(Notify::new());
    let handler = Arc::new(Dispatcher {
        state: state.clone(),
        endpoint: endpoint.clone(),
        shutdown: shutdown.clone(),
    });

    let router = iroh::protocol::Router::builder(endpoint.clone())
        .accept(
            lcp_protocol::ALPN,
            Arc::new(transport::LcpProtocolHandler::new(state.clone())),
        )
        .spawn();

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

    drop(router);
    endpoint.close().await;

    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(&socket_path);
    }

    Ok(())
}
