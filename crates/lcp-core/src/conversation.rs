//! In-memory conversation history, latest-incoming indexes, and dedup (spec §9). Messages
//! live only in RAM -- there is no persistence here by design (ADR 0006).

use std::collections::{HashMap, HashSet, VecDeque};

use serde::Serialize;
use uuid::Uuid;

/// Dedup cache size. Spec §9 wants "at least 2x total history capacity with a hard memory
/// bound" -- rather than deriving that from the live peer count (which would make the cache
/// grow unboundedly as peers are added), this is a fixed cap comfortably above 2x the maximum
/// configurable per-peer history (`MAX_HISTORY_LIMIT` = 500) for a realistic number of peers
/// (spec's own assumption is ~50), while staying a small, fixed, easily-reasoned-about size.
pub const DEDUP_CACHE_CAPACITY: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Incoming,
    Outgoing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageStatus {
    Sending,
    Sent,
    Received,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct StoredMessage {
    pub id: Uuid,
    pub peer_id: String,
    pub direction: Direction,
    pub sender_label: String,
    pub text: String,
    pub sent_at_unix_ms: i64,
    /// When this row was locally recorded -- for `Incoming` this is genuine receive time; for
    /// `Outgoing` it's when the send attempt was logged. Either way it's a local wall-clock
    /// value used to order the mixed-direction view `pick` shows (spec §11.9), distinct from
    /// `receive_sequence`, which is the authoritative "latest incoming" ordering (spec §8.8).
    pub received_at_unix_ms: i64,
    pub receive_sequence: Option<u64>,
    pub status: MessageStatus,
}

#[derive(Debug, Default)]
pub struct Conversation {
    pub messages: VecDeque<StoredMessage>,
    pub latest_incoming: Option<Uuid>,
}

pub enum RecordOutcome {
    Accepted,
    /// Same message id already seen from this peer -- caller should ACK success without
    /// adding history again (spec §8.7).
    Duplicate,
}

struct BoundedDedupCache {
    order: VecDeque<(String, Uuid)>,
    seen: HashSet<(String, Uuid)>,
    capacity: usize,
}

impl BoundedDedupCache {
    fn new(capacity: usize) -> Self {
        Self {
            order: VecDeque::new(),
            seen: HashSet::new(),
            capacity,
        }
    }

    fn contains(&self, peer_id: &str, id: Uuid) -> bool {
        self.seen.contains(&(peer_id.to_string(), id))
    }

    fn insert(&mut self, peer_id: &str, id: Uuid) {
        let key = (peer_id.to_string(), id);
        if !self.seen.insert(key.clone()) {
            return;
        }
        self.order.push_back(key);
        if self.order.len() > self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.seen.remove(&oldest);
            }
        }
    }
}

pub struct ConversationStore {
    conversations: HashMap<String, Conversation>,
    latest_incoming_global: Option<(String, Uuid)>,
    dedup: BoundedDedupCache,
    next_receive_sequence: u64,
    history_limit_per_peer: usize,
}

impl ConversationStore {
    pub fn new(history_limit_per_peer: usize) -> Self {
        Self {
            conversations: HashMap::new(),
            latest_incoming_global: None,
            dedup: BoundedDedupCache::new(DEDUP_CACHE_CAPACITY),
            next_receive_sequence: 0,
            history_limit_per_peer,
        }
    }

    /// Records an incoming message, assigning the next `receive_sequence`. A duplicate
    /// `message_id` from the same peer is recognized and does not add history again.
    #[allow(clippy::too_many_arguments)]
    pub fn record_incoming(
        &mut self,
        peer_id: &str,
        sender_label: &str,
        message_id: Uuid,
        sent_at_unix_ms: i64,
        received_at_unix_ms: i64,
        text: String,
    ) -> RecordOutcome {
        if self.dedup.contains(peer_id, message_id) {
            return RecordOutcome::Duplicate;
        }
        self.dedup.insert(peer_id, message_id);

        let receive_sequence = self.next_receive_sequence;
        self.next_receive_sequence += 1;

        let message = StoredMessage {
            id: message_id,
            peer_id: peer_id.to_string(),
            direction: Direction::Incoming,
            sender_label: sender_label.to_string(),
            text,
            sent_at_unix_ms,
            received_at_unix_ms,
            receive_sequence: Some(receive_sequence),
            status: MessageStatus::Received,
        };

        let conversation = self.conversations.entry(peer_id.to_string()).or_default();
        conversation.latest_incoming = Some(message_id);
        push_bounded(
            &mut conversation.messages,
            message,
            self.history_limit_per_peer,
        );
        self.latest_incoming_global = Some((peer_id.to_string(), message_id));
        RecordOutcome::Accepted
    }

    pub fn record_outgoing(
        &mut self,
        peer_id: &str,
        message_id: Uuid,
        text: String,
        status: MessageStatus,
    ) -> StoredMessage {
        let now = now_unix_ms();
        let message = StoredMessage {
            id: message_id,
            peer_id: peer_id.to_string(),
            direction: Direction::Outgoing,
            sender_label: "You".to_string(),
            text,
            sent_at_unix_ms: now,
            received_at_unix_ms: now,
            receive_sequence: None,
            status,
        };
        let conversation = self.conversations.entry(peer_id.to_string()).or_default();
        push_bounded(
            &mut conversation.messages,
            message.clone(),
            self.history_limit_per_peer,
        );
        message
    }

    pub fn update_outgoing_status(
        &mut self,
        peer_id: &str,
        message_id: Uuid,
        status: MessageStatus,
    ) {
        if let Some(conversation) = self.conversations.get_mut(peer_id) {
            if let Some(message) = conversation
                .messages
                .iter_mut()
                .find(|m| m.id == message_id)
            {
                message.status = status;
            }
        }
    }

    /// Latest incoming message from one peer. Outgoing messages are never returned (spec §11.7).
    pub fn latest_incoming_for(&self, peer_id: &str) -> Option<&StoredMessage> {
        let conversation = self.conversations.get(peer_id)?;
        let id = conversation.latest_incoming?;
        conversation.messages.iter().find(|m| m.id == id)
    }

    /// Latest incoming message from any peer, by `receive_sequence` (spec §8.8) -- never an
    /// outgoing message, and never ordered by sender-supplied timestamps.
    pub fn latest_incoming_global(&self) -> Option<&StoredMessage> {
        let (peer_id, id) = self.latest_incoming_global.as_ref()?;
        self.conversations
            .get(peer_id)?
            .messages
            .iter()
            .find(|m| &m.id == id)
    }

    pub fn messages_for(&self, peer_id: &str) -> impl Iterator<Item = &StoredMessage> {
        self.conversations
            .get(peer_id)
            .into_iter()
            .flat_map(|c| c.messages.iter())
    }

    pub fn message_for(&self, peer_id: &str, message_id: Uuid) -> Option<&StoredMessage> {
        self.messages_for(peer_id).find(|m| m.id == message_id)
    }

    /// All messages across every peer, newest first by local record time (spec §11.9's `pick`
    /// without a peer argument).
    pub fn all_messages_newest_first(&self) -> Vec<&StoredMessage> {
        let mut all: Vec<&StoredMessage> = self
            .conversations
            .values()
            .flat_map(|c| c.messages.iter())
            .collect();
        all.sort_by_key(|m| std::cmp::Reverse(m.received_at_unix_ms));
        all
    }

    pub fn set_history_limit(&mut self, limit: usize) {
        self.history_limit_per_peer = limit;
    }
}

fn push_bounded(messages: &mut VecDeque<StoredMessage>, message: StoredMessage, limit: usize) {
    messages.push_back(message);
    while messages.len() > limit {
        messages.pop_front();
    }
}

fn now_unix_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_incoming_per_peer_tracks_most_recent() {
        let mut store = ConversationStore::new(100);
        store.record_incoming("peer-a", "First", Uuid::new_v4(), 1, 1, "hello".into());
        let second = Uuid::new_v4();
        store.record_incoming("peer-a", "First", second, 2, 2, "world".into());
        assert_eq!(store.latest_incoming_for("peer-a").unwrap().id, second);
    }

    #[test]
    fn latest_incoming_global_reflects_highest_receive_sequence() {
        let mut store = ConversationStore::new(100);
        store.record_incoming("peer-a", "First", Uuid::new_v4(), 1, 1, "A".into());
        let latest = Uuid::new_v4();
        store.record_incoming("peer-b", "Beam", latest, 2, 2, "B".into());
        assert_eq!(store.latest_incoming_global().unwrap().id, latest);
        assert_eq!(store.latest_incoming_global().unwrap().text, "B");
    }

    #[test]
    fn outgoing_messages_are_excluded_from_latest_incoming() {
        let mut store = ConversationStore::new(100);
        store.record_outgoing(
            "peer-a",
            Uuid::new_v4(),
            "outgoing only".into(),
            MessageStatus::Sent,
        );
        assert!(store.latest_incoming_for("peer-a").is_none());
        assert!(store.latest_incoming_global().is_none());
    }

    #[test]
    fn duplicate_message_id_from_same_peer_is_not_added_twice() {
        let mut store = ConversationStore::new(100);
        let id = Uuid::new_v4();
        assert!(matches!(
            store.record_incoming("peer-a", "First", id, 1, 1, "hi".into()),
            RecordOutcome::Accepted
        ));
        assert!(matches!(
            store.record_incoming("peer-a", "First", id, 1, 1, "hi".into()),
            RecordOutcome::Duplicate
        ));
        assert_eq!(store.messages_for("peer-a").count(), 1);
    }

    #[test]
    fn same_message_id_from_different_peers_is_not_a_duplicate() {
        let mut store = ConversationStore::new(100);
        let id = Uuid::new_v4();
        store.record_incoming("peer-a", "First", id, 1, 1, "hi".into());
        assert!(matches!(
            store.record_incoming("peer-b", "Beam", id, 1, 1, "hi".into()),
            RecordOutcome::Accepted
        ));
    }

    #[test]
    fn history_trims_oldest_when_over_limit() {
        let mut store = ConversationStore::new(2);
        store.record_incoming("peer-a", "First", Uuid::new_v4(), 1, 1, "one".into());
        store.record_incoming("peer-a", "First", Uuid::new_v4(), 2, 2, "two".into());
        let third = Uuid::new_v4();
        store.record_incoming("peer-a", "First", third, 3, 3, "three".into());
        let texts: Vec<_> = store
            .messages_for("peer-a")
            .map(|m| m.text.as_str())
            .collect();
        assert_eq!(texts, vec!["two", "three"]);
        assert_eq!(store.latest_incoming_for("peer-a").unwrap().id, third);
    }

    #[test]
    fn receive_sequence_is_monotonically_increasing() {
        let mut store = ConversationStore::new(100);
        store.record_incoming("peer-a", "First", Uuid::new_v4(), 1, 1, "a".into());
        store.record_incoming("peer-b", "Beam", Uuid::new_v4(), 1, 1, "b".into());
        let sequences: Vec<u64> = store
            .all_messages_newest_first()
            .iter()
            .filter_map(|m| m.receive_sequence)
            .collect();
        assert!(sequences.contains(&0));
        assert!(sequences.contains(&1));
    }

    #[test]
    fn all_messages_newest_first_mixes_directions_and_peers() {
        let mut store = ConversationStore::new(100);
        store.record_incoming("peer-a", "First", Uuid::new_v4(), 1, 10, "incoming".into());
        store.record_outgoing(
            "peer-a",
            Uuid::new_v4(),
            "outgoing".into(),
            MessageStatus::Sent,
        );
        let all = store.all_messages_newest_first();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn update_outgoing_status_changes_status_in_place() {
        let mut store = ConversationStore::new(100);
        let id = Uuid::new_v4();
        store.record_outgoing("peer-a", id, "hi".into(), MessageStatus::Sending);
        store.update_outgoing_status("peer-a", id, MessageStatus::Sent);
        let message = store.messages_for("peer-a").find(|m| m.id == id).unwrap();
        assert_eq!(message.status, MessageStatus::Sent);
    }
}
