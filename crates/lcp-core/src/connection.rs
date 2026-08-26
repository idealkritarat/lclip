//! Connection caching and online/path status (spec §8.2, §8.9). This is the lighter half of
//! Phase 4: sends reuse a still-open connection instead of always dialing fresh, and status
//! reflects real recent connectivity. A continuously-running background reconnect loop with
//! the full backoff schedule (spec §8.10) is not implemented yet -- status is updated
//! opportunistically (on send attempts and on incoming connections) rather than proactively,
//! which is simpler and already gives accurate, if not always immediately fresh, status.

use std::collections::HashMap;

use iroh::endpoint::Connection;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PeerStatus {
    Online,
    Connecting,
    Offline,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PathType {
    Direct,
    Relay,
    #[default]
    Unknown,
}

#[derive(Default)]
struct PeerRuntimeState {
    status: PeerStatus,
    path: PathType,
    connection: Option<Connection>,
}

#[derive(Default)]
pub struct ConnectionRegistry {
    peers: HashMap<String, PeerRuntimeState>,
}

impl ConnectionRegistry {
    pub fn status_of(&self, endpoint_id: &str) -> (PeerStatus, PathType) {
        match self.peers.get(endpoint_id) {
            Some(s)
                if s.connection
                    .as_ref()
                    .is_some_and(|c| c.close_reason().is_none()) =>
            {
                (PeerStatus::Online, s.path)
            }
            Some(s) => (s.status, PathType::Unknown),
            None => (PeerStatus::Unknown, PathType::Unknown),
        }
    }

    /// A cached connection to `endpoint_id`, if one exists and hasn't closed.
    pub fn cached_connection(&self, endpoint_id: &str) -> Option<Connection> {
        let state = self.peers.get(endpoint_id)?;
        let connection = state.connection.as_ref()?;
        if connection.close_reason().is_some() {
            return None;
        }
        Some(connection.clone())
    }

    pub fn mark_online(&mut self, endpoint_id: &str, connection: Connection) {
        let entry = self.peers.entry(endpoint_id.to_string()).or_default();
        entry.status = PeerStatus::Online;
        entry.connection = Some(connection);
    }

    /// Closes and forgets any cached connection to `endpoint_id` (spec §7.8: unpair must close
    /// the active connection immediately, not just stop trusting future ones).
    pub fn close_and_forget(&mut self, endpoint_id: &str) {
        if let Some(state) = self.peers.remove(endpoint_id) {
            if let Some(connection) = state.connection {
                connection.close(0u32.into(), b"unpaired");
            }
        }
    }

    pub fn mark_offline(&mut self, endpoint_id: &str) {
        let entry = self.peers.entry(endpoint_id.to_string()).or_default();
        entry.status = PeerStatus::Offline;
        entry.path = PathType::Unknown;
        entry.connection = None;
    }

    /// Deterministic tie-break for simultaneous connection initiation (spec §8.2): the lower
    /// `EndpointId` owns the outbound preferred connection. Called when accepting a new
    /// inbound connection from a peer we may already have an outbound connection to. Returns
    /// `true` if the new inbound connection should be kept (and any existing one replaced),
    /// `false` if the inbound connection should be closed in favor of the existing one.
    pub fn should_keep_inbound(&self, my_endpoint_id: &str, remote_endpoint_id: &str) -> bool {
        match self.cached_connection(remote_endpoint_id) {
            None => true,
            Some(_) => remote_endpoint_id < my_endpoint_id,
        }
    }
}
