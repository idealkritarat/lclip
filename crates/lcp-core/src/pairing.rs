//! Pairing session state, the human-verification string (spec §7.6, §7.7), and the network
//! exchange for both roles (spec §7.5): the inviter's accept-side handler, the joiner's
//! initiate-side call, and the two-sided confirmation exchange they share.

use std::time::{Duration, Instant, SystemTime};

use iroh::endpoint::{RecvStream, SendStream};
use subtle::ConstantTimeEq;
use tokio::sync::oneshot;
use uuid::Uuid;

use lcp_protocol::ipc::events;
use lcp_protocol::network::{NetworkBody, NetworkEnvelope, PairConfirm, PairDecision, PairRequest};
use lcp_protocol::ticket::PairingTicketV1;

use crate::peers;
use crate::state::SharedState;
use crate::transport::{now_unix_ms, read_envelope, write_envelope, TransportError};

pub const PAIRING_CONTEXT: &[u8] = b"lcp-pairing-v1";
pub const PAIRING_CONFIRM_TIMEOUT: Duration = Duration::from_secs(120);

/// A short, curated list (not a security boundary -- see module docs) used only to render a
/// human-checkable string. 64 entries so a byte maps onto it with a clean, low-bias modulo.
const WORDLIST: [&str; 64] = [
    "mango", "river", "pencil", "orbit", "cedar", "flame", "cobalt", "harbor", "meadow", "quartz",
    "silver", "timber", "violet", "amber", "canyon", "delta", "ember", "frost", "granite",
    "hollow", "ivory", "jungle", "kernel", "lagoon", "marble", "nectar", "opal", "prairie",
    "quiver", "ridge", "summit", "tundra", "umber", "valley", "willow", "xenon", "yonder",
    "zephyr", "anchor", "basalt", "cinder", "dune", "echo", "fable", "glacier", "haven", "island",
    "jasper", "knoll", "lumen", "mesa", "nimbus", "onyx", "pebble", "quill", "raven", "shard",
    "thicket", "unity", "vapor", "wisp", "yarrow", "zenith", "arbor",
];

/// Derives the same verification string on both sides regardless of role (inviter/joiner) or
/// argument order -- endpoint ids and nonces are sorted internally before hashing. Never use
/// this string as key material (spec §14.2); it exists purely for a human to eyeball.
pub fn derive_verification_string(
    endpoint_id_a: &str,
    endpoint_id_b: &str,
    invite_secret: &[u8; 32],
    nonce_a: &[u8; 32],
    nonce_b: &[u8; 32],
) -> String {
    let (id1, id2) = if endpoint_id_a <= endpoint_id_b {
        (endpoint_id_a, endpoint_id_b)
    } else {
        (endpoint_id_b, endpoint_id_a)
    };
    let (n1, n2) = if nonce_a <= nonce_b {
        (nonce_a, nonce_b)
    } else {
        (nonce_b, nonce_a)
    };

    let mut hasher = blake3::Hasher::new();
    hasher.update(PAIRING_CONTEXT);
    hasher.update(id1.as_bytes());
    hasher.update(id2.as_bytes());
    hasher.update(invite_secret);
    hasher.update(n1);
    hasher.update(n2);
    let hash = hasher.finalize();
    let bytes = hash.as_bytes();

    let w1 = WORDLIST[bytes[0] as usize % WORDLIST.len()];
    let w2 = WORDLIST[bytes[1] as usize % WORDLIST.len()];
    let w3 = WORDLIST[bytes[2] as usize % WORDLIST.len()];
    let digits = u16::from_be_bytes([bytes[3], bytes[4]]) % 10000;
    format!("{w1}-{w2}-{w3}-{digits:04}")
}

/// Constant-time invite-secret comparison (spec §14.2) -- never compare secrets with `==`.
pub fn secrets_match(a: &[u8; 32], b: &[u8; 32]) -> bool {
    a.ct_eq(b).into()
}

/// 32 CSPRNG bytes, used for both invite secrets and pairing nonces (spec §14.2). The OS
/// randomness source failing is an unrecoverable condition, not something callers can sensibly
/// handle, so this panics rather than threading a `Result` through every caller.
pub fn random_secret() -> [u8; 32] {
    let mut buf = [0u8; 32];
    getrandom::fill(&mut buf).expect("OS random number generator is unavailable");
    buf
}

fn now_rfc3339() -> String {
    chrono::DateTime::<chrono::Utc>::from(SystemTime::now()).to_rfc3339()
}

pub struct ActiveInvite {
    pub invite_secret: [u8; 32],
    pub created_at: Instant,
    pub expires_at: Instant,
    pub ticket_text: String,
}

impl ActiveInvite {
    pub fn is_expired(&self, now: Instant) -> bool {
        now >= self.expires_at
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PairingRole {
    Inviter,
    Joiner,
}

/// One in-flight pairing session, from the moment a secret is verified / a join is initiated
/// until both sides confirm or it fails. `local_decision_tx` is consumed exactly once, by
/// whichever `confirm_pairing`/`reject_pairing` call answers it first.
pub struct PendingPairing {
    pub role: PairingRole,
    pub peer_endpoint_id: String,
    pub peer_display_name: String,
    pub peer_device_name: String,
    pub verification_string: String,
    pub local_decision_tx: Option<oneshot::Sender<bool>>,
}

/// Inviter side: handles one `PairRequest` arriving on a freshly-opened stream (spec §7.5
/// steps 5-11). Runs entirely in the task spawned by [`crate::transport::LcpProtocolHandler`].
pub(crate) async fn handle_pair_request(
    state: &SharedState,
    joiner_endpoint_id: &str,
    req: PairRequest,
    mut send: SendStream,
    mut recv: RecvStream,
) -> Result<(), TransportError> {
    let now = Instant::now();
    let invite_ok = {
        let state = state.read().await;
        state.active_invites.iter().any(|inv| {
            secrets_match(&inv.invite_secret, &req.invite_secret) && !inv.is_expired(now)
        })
    };
    if !invite_ok {
        let decision = NetworkEnvelope::new(
            Uuid::new_v4(),
            NetworkBody::PairDecision(PairDecision {
                accepted: false,
                inviter_nonce: [0u8; 32],
                inviter_display_name: String::new(),
                inviter_device_name: String::new(),
                error_code: Some("invalid_or_expired_invite".into()),
            }),
        );
        let _ = write_envelope(&mut send, &decision).await;
        let _ = send.finish();
        return Err(TransportError::msg(format!(
            "pairing request with invalid/expired invite secret from {joiner_endpoint_id}"
        )));
    }

    let (my_endpoint_id, my_display_name, my_device_name) = {
        let state = state.read().await;
        (
            state.identity.endpoint_id().to_string(),
            state.config.user.name.clone(),
            state.config.user.device_name.clone(),
        )
    };

    let inviter_nonce = random_secret();
    let verification_string = derive_verification_string(
        &my_endpoint_id,
        joiner_endpoint_id,
        &req.invite_secret,
        &inviter_nonce,
        &req.joiner_nonce,
    );

    let pairing_id = Uuid::new_v4();
    let (local_tx, local_rx) = oneshot::channel();
    {
        let mut state = state.write().await;
        state.pending_pairings.insert(
            pairing_id,
            PendingPairing {
                role: PairingRole::Inviter,
                peer_endpoint_id: joiner_endpoint_id.to_string(),
                peer_display_name: req.joiner_display_name.clone(),
                peer_device_name: req.joiner_device_name.clone(),
                verification_string: verification_string.clone(),
                local_decision_tx: Some(local_tx),
            },
        );
        state.broadcast_event(
            events::PAIRING_REQUESTED,
            serde_json::json!({
                "pairing_id": pairing_id,
                "peer_display_name": req.joiner_display_name,
                "peer_device_name": req.joiner_device_name,
                "verification_string": verification_string,
            }),
        );
    }

    let decision = NetworkEnvelope::new(
        Uuid::new_v4(),
        NetworkBody::PairDecision(PairDecision {
            accepted: true,
            inviter_nonce,
            inviter_display_name: my_display_name,
            inviter_device_name: my_device_name,
            error_code: None,
        }),
    );
    write_envelope(&mut send, &decision).await?;

    let outcome = run_confirmation_exchange(&mut send, &mut recv, local_rx).await;
    finish_pairing(
        state,
        pairing_id,
        joiner_endpoint_id,
        &req.joiner_display_name,
        &req.joiner_device_name,
        Some(&req.invite_secret),
        outcome,
    )
    .await;
    Ok(())
}

/// Joiner side: dials the inviter from a decoded ticket, sends the `PairRequest`, and returns
/// the verification string for the CLI to show immediately. The confirmation exchange runs in
/// a background task so the `join_invite` IPC call itself returns quickly (spec §10.6 -- the
/// CLI subscribes and waits for `pairing_updated`).
pub async fn join_pairing(
    endpoint: &iroh::Endpoint,
    state: &SharedState,
    ticket_text: &str,
) -> Result<(Uuid, String), TransportError> {
    let ticket = PairingTicketV1::decode(ticket_text)?;
    if ticket.is_expired(now_unix_ms()) {
        return Err(TransportError::msg("ticket has expired"));
    }

    let endpoint_ticket: iroh_tickets::endpoint::EndpointTicket = ticket
        .endpoint_ticket
        .parse()
        .map_err(|_| TransportError::msg("malformed endpoint dialing info in ticket"))?;
    let endpoint_addr: iroh::EndpointAddr = endpoint_ticket.into();

    let connection = tokio::time::timeout(
        lcp_protocol::network::CONNECT_TIMEOUT,
        endpoint.connect(endpoint_addr, lcp_protocol::ALPN),
    )
    .await??;

    let (mut send, mut recv) = connection
        .open_bi()
        .await
        .map_err(|e| TransportError::msg(e.to_string()))?;

    let joiner_nonce = random_secret();
    let (my_id, my_name, my_device) = {
        let state = state.read().await;
        (
            state.identity.endpoint_id().to_string(),
            state.config.user.name.clone(),
            state.config.user.device_name.clone(),
        )
    };

    let request = NetworkEnvelope::new(
        Uuid::new_v4(),
        NetworkBody::PairRequest(PairRequest {
            invite_secret: ticket.invite_secret,
            joiner_endpoint_id: my_id.clone(),
            joiner_display_name: my_name,
            joiner_device_name: my_device,
            joiner_nonce,
        }),
    );
    write_envelope(&mut send, &request).await?;

    let response = tokio::time::timeout(
        lcp_protocol::network::SEND_TIMEOUT,
        read_envelope(&mut recv),
    )
    .await??;
    let decision = match response.body {
        NetworkBody::PairDecision(d) => d,
        _ => {
            return Err(TransportError::msg(
                "unexpected response to pairing request",
            ))
        }
    };
    if !decision.accepted {
        return Err(TransportError::msg(
            decision
                .error_code
                .unwrap_or_else(|| "pairing request rejected".into()),
        ));
    }

    let inviter_endpoint_id = connection.remote_id().to_string();
    let verification_string = derive_verification_string(
        &my_id,
        &inviter_endpoint_id,
        &ticket.invite_secret,
        &joiner_nonce,
        &decision.inviter_nonce,
    );

    let pairing_id = Uuid::new_v4();
    let (local_tx, local_rx) = oneshot::channel();
    {
        let mut state_guard = state.write().await;
        state_guard.pending_pairings.insert(
            pairing_id,
            PendingPairing {
                role: PairingRole::Joiner,
                peer_endpoint_id: inviter_endpoint_id.clone(),
                peer_display_name: decision.inviter_display_name.clone(),
                peer_device_name: decision.inviter_device_name.clone(),
                verification_string: verification_string.clone(),
                local_decision_tx: Some(local_tx),
            },
        );
    }

    let state_for_task = state.clone();
    let inviter_display_name = decision.inviter_display_name.clone();
    let inviter_device_name = decision.inviter_device_name.clone();
    tokio::spawn(async move {
        // Keep the connection alive for the task's whole lifetime -- otherwise this function
        // returning would drop the only handle to it while the exchange below is still using
        // streams derived from it.
        let _connection = connection;
        let mut send = send;
        let mut recv = recv;
        let outcome = run_confirmation_exchange(&mut send, &mut recv, local_rx).await;
        finish_pairing(
            &state_for_task,
            pairing_id,
            &inviter_endpoint_id,
            &inviter_display_name,
            &inviter_device_name,
            None,
            outcome,
        )
        .await;
    });

    Ok((pairing_id, verification_string))
}

/// Waits (bounded by [`PAIRING_CONFIRM_TIMEOUT`]) for the local human's decision, sends our own
/// `PairConfirm`, and -- only if we confirmed -- waits for the peer's `PairConfirm` too. Trust
/// only ever commits when both sides said yes (spec §7.5 step 9).
async fn run_confirmation_exchange(
    send: &mut SendStream,
    recv: &mut RecvStream,
    local_rx: oneshot::Receiver<bool>,
) -> Result<bool, TransportError> {
    let start = Instant::now();
    let local_confirmed = matches!(
        tokio::time::timeout(PAIRING_CONFIRM_TIMEOUT, local_rx).await,
        Ok(Ok(true))
    );

    let confirm = NetworkEnvelope::new(
        Uuid::new_v4(),
        NetworkBody::PairConfirm(PairConfirm {
            confirmed: local_confirmed,
        }),
    );
    write_envelope(send, &confirm).await?;
    let _ = send.finish();

    if !local_confirmed {
        return Ok(false);
    }

    let remaining = PAIRING_CONFIRM_TIMEOUT.saturating_sub(start.elapsed());
    let remote_envelope = tokio::time::timeout(remaining, read_envelope(recv)).await??;
    match remote_envelope.body {
        NetworkBody::PairConfirm(c) => Ok(c.confirmed),
        _ => Ok(false),
    }
}

/// Commits trust and cleans up session bookkeeping once a confirmation exchange settles,
/// regardless of which side/role called it.
#[allow(clippy::too_many_arguments)]
async fn finish_pairing(
    state: &SharedState,
    pairing_id: Uuid,
    peer_endpoint_id: &str,
    peer_display_name: &str,
    peer_device_name: &str,
    consumed_invite_secret: Option<&[u8; 32]>,
    outcome: Result<bool, TransportError>,
) {
    let mut state = state.write().await;
    state.pending_pairings.remove(&pairing_id);
    if let Some(secret) = consumed_invite_secret {
        state
            .active_invites
            .retain(|inv| !secrets_match(&inv.invite_secret, secret));
    }
    match outcome {
        Ok(true) => {
            let alias = peers::add_trusted_peer(
                &mut state.config.trusted_peers,
                peer_endpoint_id.to_string(),
                peer_display_name,
                peer_device_name.to_string(),
                now_rfc3339(),
            );
            if let Err(e) = state.config.save() {
                tracing::warn!(error = %e, "failed to persist newly trusted peer");
            }
            state.broadcast_event(
                events::PAIRING_UPDATED,
                serde_json::json!({"pairing_id": pairing_id, "status": "paired", "alias": alias}),
            );
        }
        Ok(false) => {
            state.broadcast_event(
                events::PAIRING_UPDATED,
                serde_json::json!({"pairing_id": pairing_id, "status": "rejected"}),
            );
        }
        Err(e) => {
            state.broadcast_event(
                events::PAIRING_UPDATED,
                serde_json::json!({"pairing_id": pairing_id, "status": "failed", "reason": e.to_string()}),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verification_string_is_order_independent() {
        let secret = [1u8; 32];
        let nonce_a = [2u8; 32];
        let nonce_b = [3u8; 32];
        let from_inviter = derive_verification_string("aaa", "bbb", &secret, &nonce_a, &nonce_b);
        let from_joiner = derive_verification_string("bbb", "aaa", &secret, &nonce_b, &nonce_a);
        assert_eq!(from_inviter, from_joiner);
    }

    #[test]
    fn verification_string_changes_with_different_secret() {
        let nonce_a = [2u8; 32];
        let nonce_b = [3u8; 32];
        let s1 = derive_verification_string("aaa", "bbb", &[1u8; 32], &nonce_a, &nonce_b);
        let s2 = derive_verification_string("aaa", "bbb", &[9u8; 32], &nonce_a, &nonce_b);
        assert_ne!(s1, s2);
    }

    #[test]
    fn verification_string_has_expected_shape() {
        let s = derive_verification_string("aaa", "bbb", &[1u8; 32], &[2u8; 32], &[3u8; 32]);
        let parts: Vec<&str> = s.split('-').collect();
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[3].len(), 4);
        assert!(parts[3].chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn secrets_match_is_correct() {
        assert!(secrets_match(&[7u8; 32], &[7u8; 32]));
        assert!(!secrets_match(&[7u8; 32], &[8u8; 32]));
    }
}
