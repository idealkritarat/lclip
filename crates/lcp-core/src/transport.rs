//! Iroh endpoint binding, the ALPN protocol handler, and incoming text-message handling
//! (spec §8). Outgoing pairing network I/O lives in `pairing.rs`; connection caching and
//! status live in `connection.rs`. The proactive background reconnect loop with the full
//! backoff schedule (spec §8.10) is not implemented yet -- status/caching today are
//! opportunistic (updated on send attempts and incoming connections), not continuously
//! maintained in the background.

use std::time::{SystemTime, UNIX_EPOCH};

use iroh::endpoint::{Connection, RecvStream, SendStream};
use iroh::protocol::{AcceptError, ProtocolHandler};
use uuid::Uuid;

use lcp_protocol::ipc::events;
use lcp_protocol::network::{
    self, AckPayload, NetworkBody, NetworkEnvelope, NetworkErrorPayload, TextPayload, ACK_TIMEOUT,
    CONNECT_TIMEOUT, SEND_TIMEOUT,
};
use lcp_protocol::ALPN;

use crate::conversation::RecordOutcome;
use crate::state::{AppState, SharedState};

/// Errors from network I/O and the pairing exchange. A handful of `#[from]` conversions cover
/// the common cases; everything else that just needs a human-readable message (a rejected
/// pairing, an expired ticket, ...) goes through `Message` -- callers here only ever need to
/// show/log a string or map to one generic IPC error code, not match on fine-grained variants.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Protocol(#[from] lcp_protocol::ProtocolError),
    #[error("stream write error: {0}")]
    Write(#[from] iroh::endpoint::WriteError),
    #[error("stream read error: {0}")]
    Read(#[from] iroh::endpoint::ReadExactError),
    #[error("connect error: {0}")]
    Connect(#[from] iroh::endpoint::ConnectError),
    #[error("bind error: {0}")]
    Bind(#[from] iroh::endpoint::BindError),
    #[error("operation timed out")]
    Timeout,
    #[error("{0}")]
    Message(String),
}

impl From<tokio::time::error::Elapsed> for TransportError {
    fn from(_: tokio::time::error::Elapsed) -> Self {
        TransportError::Timeout
    }
}

impl TransportError {
    pub(crate) fn msg(text: impl Into<String>) -> Self {
        TransportError::Message(text.into())
    }
}

pub async fn bind_endpoint(secret_key: iroh::SecretKey) -> Result<iroh::Endpoint, TransportError> {
    let endpoint = iroh::Endpoint::builder(iroh::endpoint::presets::N0)
        .secret_key(secret_key)
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await?;
    Ok(endpoint)
}

/// Broadcasts the peer's current status/path to IPC subscribers (spec §13.5's realtime friend
/// list) right after a status-changing call into `peer_connections`. Callers already hold the
/// write lock this needs, so it just borrows it rather than re-acquiring.
fn broadcast_peer_status(state: &mut AppState, endpoint_id: &str) {
    let (status, path) = state.peer_connections.status_of(endpoint_id);
    state.broadcast_event(
        events::PEER_UPDATED,
        serde_json::json!({
            "endpoint_id": endpoint_id,
            "status": status,
            "path": path,
        }),
    );
}

pub(crate) fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub(crate) async fn write_envelope(
    send: &mut SendStream,
    envelope: &NetworkEnvelope,
) -> Result<(), TransportError> {
    let framed = envelope.encode_framed()?;
    send.write_all(&framed).await?;
    Ok(())
}

pub(crate) async fn read_envelope(
    recv: &mut RecvStream,
) -> Result<NetworkEnvelope, TransportError> {
    let mut header = [0u8; network::FRAME_LENGTH_PREFIX_BYTES];
    recv.read_exact(&mut header).await?;
    let len = network::decode_length_prefix(header)?;
    let mut buf = vec![0u8; len];
    recv.read_exact(&mut buf).await?;
    let envelope = NetworkEnvelope::decode_body(&buf)?;
    Ok(envelope)
}

#[derive(Clone)]
pub struct LcpProtocolHandler {
    state: SharedState,
}

impl LcpProtocolHandler {
    pub fn new(state: SharedState) -> Self {
        Self { state }
    }
}

impl std::fmt::Debug for LcpProtocolHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LcpProtocolHandler").finish_non_exhaustive()
    }
}

impl ProtocolHandler for LcpProtocolHandler {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let remote_id = connection.remote_id().to_string();

        let (my_id, is_trusted) = {
            let state = self.state.read().await;
            (
                state.identity.endpoint_id().to_string(),
                state
                    .config
                    .trusted_peers
                    .iter()
                    .any(|p| p.endpoint_id == remote_id),
            )
        };
        if is_trusted {
            let mut state = self.state.write().await;
            if state
                .peer_connections
                .should_keep_inbound(&my_id, &remote_id)
            {
                state
                    .peer_connections
                    .mark_online(&remote_id, connection.clone());
                broadcast_peer_status(&mut state, &remote_id);
            } else {
                // Our own outbound connection to this peer wins the tie-break (spec §8.2);
                // this duplicate inbound one is closed rather than left to linger.
                drop(state);
                connection.close(0u32.into(), b"duplicate connection");
                return Ok(());
            }
        }

        loop {
            let (send, recv) = match connection.accept_bi().await {
                Ok(pair) => pair,
                Err(_) => break,
            };
            let state = self.state.clone();
            let remote_id = remote_id.clone();
            tokio::spawn(async move {
                if let Err(e) = handle_incoming_stream(state, remote_id, send, recv).await {
                    tracing::debug!(error = %e, "error handling incoming stream");
                }
            });
        }
        Ok(())
    }
}

async fn handle_incoming_stream(
    state: SharedState,
    remote_id: String,
    mut send: SendStream,
    mut recv: RecvStream,
) -> Result<(), TransportError> {
    let envelope = read_envelope(&mut recv).await?;
    match envelope.body {
        NetworkBody::Text(payload) => {
            handle_text(&state, &remote_id, envelope.request_id, payload, &mut send).await
        }
        NetworkBody::Ping => {
            let pong = NetworkEnvelope::new(envelope.request_id, NetworkBody::Pong);
            write_envelope(&mut send, &pong).await?;
            let _ = send.finish();
            Ok(())
        }
        NetworkBody::PairRequest(req) => {
            crate::pairing::handle_pair_request(&state, &remote_id, req, send, recv).await
        }
        other => {
            let err = NetworkEnvelope::new(
                envelope.request_id,
                NetworkBody::Error(NetworkErrorPayload {
                    code: "unexpected_message".into(),
                    message: format!("unexpected opening message: {other:?}"),
                }),
            );
            let _ = write_envelope(&mut send, &err).await;
            let _ = send.finish();
            Ok(())
        }
    }
}

async fn handle_text(
    state: &SharedState,
    remote_id: &str,
    request_id: Uuid,
    payload: TextPayload,
    send: &mut SendStream,
) -> Result<(), TransportError> {
    let trusted_alias = {
        let state = state.read().await;
        state
            .config
            .trusted_peers
            .iter()
            .find(|p| p.endpoint_id == remote_id)
            .map(|p| p.alias.clone())
    };
    let Some(sender_label) = trusted_alias else {
        let ack = NetworkEnvelope::new(
            request_id,
            NetworkBody::Error(NetworkErrorPayload {
                code: "unauthorized".into(),
                message: "endpoint is not a trusted peer".into(),
            }),
        );
        write_envelope(send, &ack).await?;
        let _ = send.finish();
        return Err(TransportError::msg(format!(
            "rejected text from untrusted endpoint {remote_id}"
        )));
    };

    if let Err(e) = network::validate_text_len(&payload.text) {
        let ack = NetworkEnvelope::new(
            request_id,
            NetworkBody::Error(NetworkErrorPayload {
                code: "limit_exceeded".into(),
                message: e.to_string(),
            }),
        );
        write_envelope(send, &ack).await?;
        let _ = send.finish();
        return Ok(());
    }

    let outcome = {
        let mut state = state.write().await;
        let outcome = state.conversations.record_incoming(
            remote_id,
            &sender_label,
            payload.message_id,
            payload.sent_at_unix_ms,
            now_unix_ms(),
            payload.text.clone(),
        );
        if matches!(outcome, RecordOutcome::Accepted) {
            state.broadcast_event(
                events::MESSAGE_RECEIVED,
                serde_json::json!({
                    "peer_id": remote_id,
                    "sender_label": sender_label,
                    "message_id": payload.message_id,
                }),
            );
        }
        outcome
    };
    let _ = outcome;

    let ack = NetworkEnvelope::new(
        request_id,
        NetworkBody::Ack(AckPayload {
            message_id: payload.message_id,
            accepted: true,
            error_code: None,
        }),
    );
    write_envelope(send, &ack).await?;
    let _ = send.finish();
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum SendTextError {
    #[error("connect timed out or failed: {0}")]
    Connect(String),
    #[error("send timed out or failed: {0}")]
    Send(String),
    #[error("peer rejected the message: {0}")]
    Rejected(String),
}

/// Reuses a cached still-open connection to `remote_endpoint_id` if one exists (spec §8.2),
/// otherwise dials fresh; sends one text message and waits for the ACK. On any failure the
/// peer is marked offline in the registry; on a fresh successful connect it's marked online.
#[allow(clippy::too_many_arguments)]
pub async fn send_text(
    state: &SharedState,
    endpoint: &iroh::Endpoint,
    endpoint_addr: iroh::EndpointAddr,
    remote_endpoint_id: &str,
    my_endpoint_id: &str,
    message_id: Uuid,
    text: &str,
) -> Result<(), SendTextError> {
    let result = send_text_inner(
        state,
        endpoint,
        endpoint_addr,
        remote_endpoint_id,
        my_endpoint_id,
        message_id,
        text,
    )
    .await;
    if result.is_err() {
        let mut state = state.write().await;
        state.peer_connections.mark_offline(remote_endpoint_id);
        broadcast_peer_status(&mut state, remote_endpoint_id);
    }
    result
}

#[allow(clippy::too_many_arguments)]
async fn send_text_inner(
    state: &SharedState,
    endpoint: &iroh::Endpoint,
    endpoint_addr: iroh::EndpointAddr,
    remote_endpoint_id: &str,
    my_endpoint_id: &str,
    message_id: Uuid,
    text: &str,
) -> Result<(), SendTextError> {
    let cached = state
        .read()
        .await
        .peer_connections
        .cached_connection(remote_endpoint_id);
    let connection = match cached {
        Some(connection) => connection,
        None => {
            let connection =
                tokio::time::timeout(CONNECT_TIMEOUT, endpoint.connect(endpoint_addr, ALPN))
                    .await
                    .map_err(|_| SendTextError::Connect("timed out".into()))?
                    .map_err(|e| SendTextError::Connect(e.to_string()))?;
            {
                let mut state = state.write().await;
                state
                    .peer_connections
                    .mark_online(remote_endpoint_id, connection.clone());
                broadcast_peer_status(&mut state, remote_endpoint_id);
            }
            connection
        }
    };

    let (mut send, mut recv) = connection
        .open_bi()
        .await
        .map_err(|e| SendTextError::Connect(e.to_string()))?;

    let envelope = NetworkEnvelope::new(
        Uuid::new_v4(),
        NetworkBody::Text(TextPayload {
            message_id,
            sender_endpoint_id: my_endpoint_id.to_string(),
            sent_at_unix_ms: now_unix_ms(),
            text: text.to_string(),
        }),
    );

    tokio::time::timeout(SEND_TIMEOUT, write_envelope(&mut send, &envelope))
        .await
        .map_err(|_| SendTextError::Send("timed out writing message".into()))?
        .map_err(|e| SendTextError::Send(e.to_string()))?;
    let _ = send.finish();

    let response = tokio::time::timeout(ACK_TIMEOUT, read_envelope(&mut recv))
        .await
        .map_err(|_| SendTextError::Send("timed out waiting for ack".into()))?
        .map_err(|e| SendTextError::Send(e.to_string()))?;

    match response.body {
        NetworkBody::Ack(ack) if ack.accepted => Ok(()),
        NetworkBody::Ack(ack) => Err(SendTextError::Rejected(
            ack.error_code.unwrap_or_else(|| "rejected".to_string()),
        )),
        NetworkBody::Error(err) => Err(SendTextError::Rejected(err.message)),
        _ => Err(SendTextError::Rejected("unexpected response".into())),
    }
}
