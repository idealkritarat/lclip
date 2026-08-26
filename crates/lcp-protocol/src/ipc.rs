//! Local IPC wire types shared between `lanclipd` (server) and `lcp`/macOS UI (clients).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::ProtocolError;

pub const IPC_FRAME_LENGTH_PREFIX_BYTES: usize = 4;
pub const MAX_IPC_FRAME_BYTES: usize = 6 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcRequest {
    pub ipc_version: u16,
    pub id: Uuid,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcResponse {
    pub ipc_version: u16,
    pub id: Uuid,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<IpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcEvent {
    pub ipc_version: u16,
    pub event: String,
    pub payload: serde_json::Value,
}

/// A frame arriving at a client: either a response correlated to one of its own requests, or
/// an unsolicited event. `serde(untagged)` picks the right variant from the fields present
/// (`id`/`ok` vs. `event`/`payload`) without a separate wire-level discriminator.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum IpcServerFrame {
    Response(IpcResponse),
    Event(IpcEvent),
}

impl IpcRequest {
    pub fn new(ipc_version: u16, method: impl Into<String>, params: serde_json::Value) -> Self {
        Self {
            ipc_version,
            id: Uuid::new_v4(),
            method: method.into(),
            params,
        }
    }
}

impl IpcResponse {
    pub fn ok(ipc_version: u16, id: Uuid, result: serde_json::Value) -> Self {
        Self {
            ipc_version,
            id,
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(
        ipc_version: u16,
        id: Uuid,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            ipc_version,
            id,
            ok: false,
            result: None,
            error: Some(IpcError {
                code: code.into(),
                message: message.into(),
            }),
        }
    }
}

/// IPC method names (spec §10.4). Centralized so client and server never typo a method string.
pub mod methods {
    pub const HELLO: &str = "hello";
    pub const GET_STATUS: &str = "get_status";
    pub const GET_CONFIG: &str = "get_config";
    pub const SET_CONFIG: &str = "set_config";
    pub const LIST_PEERS: &str = "list_peers";
    pub const CREATE_INVITE: &str = "create_invite";
    pub const CANCEL_INVITE: &str = "cancel_invite";
    pub const JOIN_INVITE: &str = "join_invite";
    pub const CONFIRM_PAIRING: &str = "confirm_pairing";
    pub const REJECT_PAIRING: &str = "reject_pairing";
    pub const UNPAIR_PEER: &str = "unpair_peer";
    pub const SEND_TEXT: &str = "send_text";
    pub const GET_LATEST_INCOMING: &str = "get_latest_incoming";
    pub const LIST_MESSAGES: &str = "list_messages";
    pub const RETRY_MESSAGE: &str = "retry_message";
    pub const SUBSCRIBE: &str = "subscribe";
    pub const SHUTDOWN: &str = "shutdown";
    pub const RUN_DIAGNOSTICS: &str = "run_diagnostics";
}

/// IPC event names (spec §10.5).
pub mod events {
    pub const DAEMON_READY: &str = "daemon_ready";
    pub const PEER_UPDATED: &str = "peer_updated";
    pub const MESSAGE_RECEIVED: &str = "message_received";
    pub const MESSAGE_UPDATED: &str = "message_updated";
    pub const PAIRING_REQUESTED: &str = "pairing_requested";
    pub const PAIRING_UPDATED: &str = "pairing_updated";
    pub const INVITE_EXPIRED: &str = "invite_expired";
    pub const CONFIG_UPDATED: &str = "config_updated";
    pub const DIAGNOSTIC_UPDATED: &str = "diagnostic_updated";
}

/// IPC error codes used in [`IpcError::code`], stable strings clients may match on.
pub mod error_codes {
    pub const UNKNOWN_METHOD: &str = "unknown_method";
    pub const INVALID_PARAMS: &str = "invalid_params";
    pub const PEER_NOT_FOUND: &str = "peer_not_found";
    pub const PEER_AMBIGUOUS: &str = "peer_ambiguous";
    pub const PEER_OFFLINE: &str = "peer_offline";
    pub const NO_MESSAGE: &str = "no_message";
    pub const PAIRING_FAILED: &str = "pairing_failed";
    pub const LIMIT_EXCEEDED: &str = "limit_exceeded";
    pub const VERSION_MISMATCH: &str = "version_mismatch";
    pub const CREDENTIAL_STORE_FAILURE: &str = "credential_store_failure";
    pub const INTERNAL: &str = "internal";
}

/// Reads a big-endian u32 length prefix and validates it against `max_frame_bytes`. Pure --
/// the transport is responsible for actually reading `len` more bytes off the wire.
pub fn decode_length_prefix(
    header: [u8; IPC_FRAME_LENGTH_PREFIX_BYTES],
    max_frame_bytes: usize,
) -> Result<usize, ProtocolError> {
    let len = u32::from_be_bytes(header) as usize;
    if len == 0 {
        return Err(ProtocolError::EmptyFrame);
    }
    if len > max_frame_bytes {
        return Err(ProtocolError::FrameTooLarge {
            max: max_frame_bytes,
            actual: len,
        });
    }
    Ok(len)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn round_trips_request_and_response() {
        let req = IpcRequest::new(1, methods::GET_STATUS, json!({}));
        let bytes = serde_json::to_vec(&req).unwrap();
        let decoded: IpcRequest = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded.method, methods::GET_STATUS);
        assert_eq!(decoded.id, req.id);

        let resp = IpcResponse::ok(1, req.id, json!({"uptime_secs": 42}));
        let bytes = serde_json::to_vec(&resp).unwrap();
        let decoded: IpcResponse = serde_json::from_slice(&bytes).unwrap();
        assert!(decoded.ok);
        assert_eq!(decoded.result.unwrap()["uptime_secs"], 42);
    }

    #[test]
    fn error_response_carries_code_and_message() {
        let id = Uuid::new_v4();
        let resp = IpcResponse::err(1, id, error_codes::PEER_NOT_FOUND, "no such peer: Foo");
        let bytes = serde_json::to_vec(&resp).unwrap();
        let decoded: IpcResponse = serde_json::from_slice(&bytes).unwrap();
        assert!(!decoded.ok);
        assert_eq!(decoded.error.unwrap().code, error_codes::PEER_NOT_FOUND);
    }
}
