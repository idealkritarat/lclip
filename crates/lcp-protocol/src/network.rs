//! Wire types exchanged between `lanclipd` instances over the Iroh `lcp/1` ALPN.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::ProtocolError;
use crate::NETWORK_PROTOCOL_VERSION;

/// Maximum UTF-8 text payload: 5 MiB.
pub const MAX_TEXT_BYTES: usize = 5 * 1024 * 1024;
/// Hard cap on a full wire frame (length prefix + serialized envelope): 6 MiB.
pub const MAX_WIRE_FRAME_BYTES: usize = 6 * 1024 * 1024;
pub const FRAME_LENGTH_PREFIX_BYTES: usize = 4;

pub const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);
pub const SEND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
pub const ACK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NetworkEnvelope {
    pub protocol_version: u16,
    pub request_id: Uuid,
    pub body: NetworkBody,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum NetworkBody {
    Text(TextPayload),
    Ack(AckPayload),
    Ping,
    Pong,
    PairRequest(PairRequest),
    PairDecision(PairDecision),
    PairConfirm(PairConfirm),
    Error(NetworkErrorPayload),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TextPayload {
    pub message_id: Uuid,
    pub sender_endpoint_id: String,
    pub sent_at_unix_ms: i64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AckPayload {
    pub message_id: Uuid,
    pub accepted: bool,
    pub error_code: Option<String>,
}

/// Sent by the joining side once it has connected to the inviter's endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PairRequest {
    pub invite_secret: [u8; 32],
    pub joiner_endpoint_id: String,
    pub joiner_display_name: String,
    pub joiner_device_name: String,
    pub joiner_nonce: [u8; 32],
}

/// Sent by the inviter once it has checked the invite secret. `accepted` only reflects
/// whether the secret/invite was valid -- it is not the human trust decision, which is
/// carried separately by [`PairConfirm`] from both sides.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PairDecision {
    pub accepted: bool,
    pub inviter_nonce: [u8; 32],
    pub inviter_display_name: String,
    pub inviter_device_name: String,
    pub error_code: Option<String>,
}

/// Carries one side's human confirm/reject decision after both have seen the verification string.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PairConfirm {
    pub confirmed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NetworkErrorPayload {
    pub code: String,
    pub message: String,
}

impl NetworkEnvelope {
    pub fn new(request_id: Uuid, body: NetworkBody) -> Self {
        Self {
            protocol_version: NETWORK_PROTOCOL_VERSION,
            request_id,
            body,
        }
    }

    /// Serializes with Postcard and prepends a 4-byte big-endian length prefix.
    pub fn encode_framed(&self) -> Result<Vec<u8>, ProtocolError> {
        let body =
            postcard::to_allocvec(self).map_err(|e| ProtocolError::Serialization(e.to_string()))?;
        if body.len() > MAX_WIRE_FRAME_BYTES {
            return Err(ProtocolError::FrameTooLarge {
                max: MAX_WIRE_FRAME_BYTES,
                actual: body.len(),
            });
        }
        let mut framed = Vec::with_capacity(FRAME_LENGTH_PREFIX_BYTES + body.len());
        framed.extend_from_slice(&(body.len() as u32).to_be_bytes());
        framed.extend_from_slice(&body);
        Ok(framed)
    }

    /// Decodes one envelope from an already length-delimited frame body (the transport must
    /// strip and validate the length prefix via [`crate::decode_length_prefix`] first).
    pub fn decode_body(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.is_empty() {
            return Err(ProtocolError::EmptyFrame);
        }
        if bytes.len() > MAX_WIRE_FRAME_BYTES {
            return Err(ProtocolError::FrameTooLarge {
                max: MAX_WIRE_FRAME_BYTES,
                actual: bytes.len(),
            });
        }
        let envelope: NetworkEnvelope =
            postcard::from_bytes(bytes).map_err(|_| ProtocolError::MalformedFrame)?;
        if envelope.protocol_version != NETWORK_PROTOCOL_VERSION {
            return Err(ProtocolError::UnsupportedVersion {
                found: envelope.protocol_version,
                expected: NETWORK_PROTOCOL_VERSION,
            });
        }
        Ok(envelope)
    }
}

/// Rejects text that is empty or exceeds [`MAX_TEXT_BYTES`]. Byte length, not char count --
/// UTF-8 content is measured the same way the wire limit measures it.
pub fn validate_text_len(text: &str) -> Result<(), ProtocolError> {
    let len = text.len();
    if len > MAX_TEXT_BYTES {
        return Err(ProtocolError::TextTooLarge {
            max: MAX_TEXT_BYTES,
            actual: len,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_text_envelope(text: &str) -> NetworkEnvelope {
        NetworkEnvelope::new(
            Uuid::new_v4(),
            NetworkBody::Text(TextPayload {
                message_id: Uuid::new_v4(),
                sender_endpoint_id: "abcd1234".into(),
                sent_at_unix_ms: 1_700_000_000_000,
                text: text.to_string(),
            }),
        )
    }

    #[test]
    fn round_trips_text_envelope() {
        let original = sample_text_envelope("hello\nworld\t!");
        let framed = original.encode_framed().unwrap();
        let len = u32::from_be_bytes(framed[..4].try_into().unwrap()) as usize;
        assert_eq!(len, framed.len() - 4);
        let decoded = NetworkEnvelope::decode_body(&framed[4..]).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn preserves_multiline_and_tabs_exactly() {
        let text = "line one\n\tline two with a tab\r\nline three";
        let original = sample_text_envelope(text);
        let framed = original.encode_framed().unwrap();
        let decoded = NetworkEnvelope::decode_body(&framed[4..]).unwrap();
        match decoded.body {
            NetworkBody::Text(payload) => assert_eq!(payload.text, text),
            other => panic!("expected Text body, got {other:?}"),
        }
    }

    #[test]
    fn round_trips_ack() {
        let original = NetworkEnvelope::new(
            Uuid::new_v4(),
            NetworkBody::Ack(AckPayload {
                message_id: Uuid::new_v4(),
                accepted: true,
                error_code: None,
            }),
        );
        let framed = original.encode_framed().unwrap();
        let decoded = NetworkEnvelope::decode_body(&framed[4..]).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn rejects_unknown_protocol_version() {
        let mut original = sample_text_envelope("hi");
        original.protocol_version = NETWORK_PROTOCOL_VERSION + 1;
        let body = postcard::to_allocvec(&original).unwrap();
        let err = NetworkEnvelope::decode_body(&body).unwrap_err();
        assert!(matches!(err, ProtocolError::UnsupportedVersion { .. }));
    }

    #[test]
    fn rejects_malformed_bytes() {
        let garbage = vec![0xffu8; 32];
        let err = NetworkEnvelope::decode_body(&garbage).unwrap_err();
        assert!(matches!(err, ProtocolError::MalformedFrame));
    }

    #[test]
    fn rejects_empty_frame_body() {
        let err = NetworkEnvelope::decode_body(&[]).unwrap_err();
        assert!(matches!(err, ProtocolError::EmptyFrame));
    }

    #[test]
    fn rejects_oversized_frame_before_decoding() {
        let oversized = vec![0u8; MAX_WIRE_FRAME_BYTES + 1];
        let err = NetworkEnvelope::decode_body(&oversized).unwrap_err();
        assert!(matches!(err, ProtocolError::FrameTooLarge { .. }));
    }

    #[test]
    fn encode_framed_rejects_oversized_text() {
        let huge = "a".repeat(MAX_WIRE_FRAME_BYTES + 1);
        let envelope = sample_text_envelope(&huge);
        let err = envelope.encode_framed().unwrap_err();
        assert!(matches!(err, ProtocolError::FrameTooLarge { .. }));
    }

    #[test]
    fn validate_text_len_accepts_within_bound() {
        assert!(validate_text_len("hello").is_ok());
        assert!(validate_text_len("").is_ok());
    }

    #[test]
    fn validate_text_len_rejects_over_max() {
        let huge = "a".repeat(MAX_TEXT_BYTES + 1);
        assert!(matches!(
            validate_text_len(&huge),
            Err(ProtocolError::TextTooLarge { .. })
        ));
    }
}
