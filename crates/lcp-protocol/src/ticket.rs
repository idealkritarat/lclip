//! Application-specific pairing ticket (spec §7.3). Encodes Iroh dialing information plus a
//! random invite secret, an expiry, and non-sensitive display metadata.

use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::error::ProtocolError;

pub const TICKET_PREFIX: &str = "lcp1_";
pub const CURRENT_TICKET_VERSION: u8 = 1;

pub const MIN_INVITE_TTL_SECS: u64 = 60;
pub const MAX_INVITE_TTL_SECS: u64 = 900;
pub const DEFAULT_INVITE_TTL_SECS: u64 = 300;

/// Generous upper bound on encoded ticket text length, rejected before any decoding is
/// attempted so a hostile/garbage argument can't drive unbounded work.
pub const MAX_TICKET_TEXT_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PairingTicketV1 {
    pub version: u8,
    pub endpoint_ticket: String,
    pub invite_secret: [u8; 32],
    pub expires_at_unix_ms: i64,
    pub inviter_display_name: String,
    pub inviter_device_name: String,
}

impl PairingTicketV1 {
    pub fn new(
        endpoint_ticket: String,
        invite_secret: [u8; 32],
        expires_at_unix_ms: i64,
        inviter_display_name: String,
        inviter_device_name: String,
    ) -> Self {
        Self {
            version: CURRENT_TICKET_VERSION,
            endpoint_ticket,
            invite_secret,
            expires_at_unix_ms,
            inviter_display_name,
            inviter_device_name,
        }
    }

    pub fn encode(&self) -> Result<String, ProtocolError> {
        let bytes =
            postcard::to_allocvec(self).map_err(|e| ProtocolError::Serialization(e.to_string()))?;
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
        Ok(format!("{TICKET_PREFIX}{encoded}"))
    }

    /// Parses and validates a ticket's shape/version. Does not check expiry -- callers must
    /// call [`Self::is_expired`] against their own clock before acting on the ticket.
    pub fn decode(ticket: &str) -> Result<Self, ProtocolError> {
        if ticket.len() > MAX_TICKET_TEXT_BYTES {
            return Err(ProtocolError::InvalidTicket("ticket text too large".into()));
        }
        let payload = ticket
            .strip_prefix(TICKET_PREFIX)
            .ok_or_else(|| ProtocolError::InvalidTicket("missing lcp1_ prefix".into()))?;
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|_| ProtocolError::InvalidTicket("invalid base64".into()))?;
        let ticket: PairingTicketV1 = postcard::from_bytes(&bytes)
            .map_err(|_| ProtocolError::InvalidTicket("malformed ticket contents".into()))?;
        if ticket.version != CURRENT_TICKET_VERSION {
            return Err(ProtocolError::InvalidTicket(format!(
                "unsupported ticket version {}",
                ticket.version
            )));
        }
        Ok(ticket)
    }

    pub fn is_expired(&self, now_unix_ms: i64) -> bool {
        now_unix_ms >= self.expires_at_unix_ms
    }
}

/// Clamps a requested invite TTL (seconds) into the allowed [`MIN_INVITE_TTL_SECS`],
/// [`MAX_INVITE_TTL_SECS`] range. MVP never allows configuring above the hard cap.
pub fn clamp_invite_ttl_secs(requested: u64) -> u64 {
    requested.clamp(MIN_INVITE_TTL_SECS, MAX_INVITE_TTL_SECS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> PairingTicketV1 {
        PairingTicketV1::new(
            "iroh-endpoint-ticket-opaque-blob".into(),
            [7u8; 32],
            1_800_000_000_000,
            "First".into(),
            "First-PC".into(),
        )
    }

    #[test]
    fn round_trips_through_text_encoding() {
        let ticket = sample();
        let text = ticket.encode().unwrap();
        assert!(text.starts_with(TICKET_PREFIX));
        let decoded = PairingTicketV1::decode(&text).unwrap();
        assert_eq!(decoded, ticket);
    }

    #[test]
    fn rejects_missing_prefix() {
        let err = PairingTicketV1::decode("not-a-ticket").unwrap_err();
        assert!(matches!(err, ProtocolError::InvalidTicket(_)));
    }

    #[test]
    fn rejects_malformed_base64() {
        let err = PairingTicketV1::decode("lcp1_***not-base64***").unwrap_err();
        assert!(matches!(err, ProtocolError::InvalidTicket(_)));
    }

    #[test]
    fn rejects_oversized_ticket_text() {
        let huge = format!("{TICKET_PREFIX}{}", "a".repeat(MAX_TICKET_TEXT_BYTES + 1));
        let err = PairingTicketV1::decode(&huge).unwrap_err();
        assert!(matches!(err, ProtocolError::InvalidTicket(_)));
    }

    #[test]
    fn rejects_unknown_version() {
        let mut ticket = sample();
        ticket.version = 99;
        let bytes = postcard::to_allocvec(&ticket).unwrap();
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
        let text = format!("{TICKET_PREFIX}{encoded}");
        let err = PairingTicketV1::decode(&text).unwrap_err();
        assert!(matches!(err, ProtocolError::InvalidTicket(_)));
    }

    #[test]
    fn expiry_is_checked_against_caller_clock() {
        let ticket = sample();
        assert!(!ticket.is_expired(1_700_000_000_000));
        assert!(ticket.is_expired(1_900_000_000_000));
    }

    #[test]
    fn ttl_is_clamped_to_bounds() {
        assert_eq!(clamp_invite_ttl_secs(10), MIN_INVITE_TTL_SECS);
        assert_eq!(clamp_invite_ttl_secs(10_000), MAX_INVITE_TTL_SECS);
        assert_eq!(clamp_invite_ttl_secs(300), 300);
    }
}
