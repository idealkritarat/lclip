//! In-memory application state owned exclusively by `lanclipd` (spec §9).

use std::sync::Arc;
use std::time::Instant;

use tokio::sync::RwLock;

use crate::config::Config;
use crate::conversation::ConversationStore;
use crate::identity::LocalIdentity;

pub struct AppState {
    pub identity: LocalIdentity,
    pub config: Config,
    pub conversations: ConversationStore,
    pub started_at: Instant,
}

impl AppState {
    pub fn new(identity: LocalIdentity, config: Config) -> Self {
        let history_limit = config.history.limit_per_peer as usize;
        Self {
            identity,
            config,
            conversations: ConversationStore::new(history_limit),
            started_at: Instant::now(),
        }
    }

    pub fn uptime(&self) -> std::time::Duration {
        self.started_at.elapsed()
    }
}

/// All daemon state lives behind one lock, per spec §9's "concurrency-safe owner" requirement.
/// At this scale (dozens of peers, low request rate) a single `RwLock` is simpler than an
/// actor/message-passing layer and is not a bottleneck.
pub type SharedState = Arc<RwLock<AppState>>;
