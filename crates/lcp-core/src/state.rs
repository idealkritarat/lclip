//! In-memory application state owned exclusively by `lanclipd` (spec §9).

use std::sync::Arc;
use std::time::Instant;

use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::RwLock;
use uuid::Uuid;

use lcp_protocol::ipc::IpcEvent;
use lcp_protocol::IPC_PROTOCOL_VERSION;

use crate::config::Config;
use crate::conversation::ConversationStore;
use crate::identity::LocalIdentity;
use crate::pairing::{ActiveInvite, PendingPairing};

pub struct AppState {
    pub identity: LocalIdentity,
    pub config: Config,
    pub conversations: ConversationStore,
    pub active_invites: Vec<ActiveInvite>,
    pub pending_pairings: std::collections::HashMap<Uuid, PendingPairing>,
    subscribers: Vec<UnboundedSender<IpcEvent>>,
    pub started_at: Instant,
}

impl AppState {
    pub fn new(identity: LocalIdentity, config: Config) -> Self {
        let history_limit = config.history.limit_per_peer as usize;
        Self {
            identity,
            config,
            conversations: ConversationStore::new(history_limit),
            active_invites: Vec::new(),
            pending_pairings: std::collections::HashMap::new(),
            subscribers: Vec::new(),
            started_at: Instant::now(),
        }
    }

    pub fn uptime(&self) -> std::time::Duration {
        self.started_at.elapsed()
    }

    pub fn add_subscriber(&mut self, sender: UnboundedSender<IpcEvent>) {
        self.subscribers.push(sender);
    }

    /// Sends an event to every currently-connected subscriber, dropping any whose connection
    /// has since closed (spec §10.5 events are best-effort).
    pub fn broadcast_event(&mut self, event: &str, payload: serde_json::Value) {
        let ipc_event = IpcEvent {
            ipc_version: IPC_PROTOCOL_VERSION,
            event: event.to_string(),
            payload,
        };
        self.subscribers
            .retain(|tx| tx.send(ipc_event.clone()).is_ok());
    }
}

/// All daemon state lives behind one lock, per spec §9's "concurrency-safe owner" requirement.
/// At this scale (dozens of peers, low request rate) a single `RwLock` is simpler than an
/// actor/message-passing layer and is not a bottleneck.
pub type SharedState = Arc<RwLock<AppState>>;
