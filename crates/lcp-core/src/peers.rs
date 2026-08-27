//! Trusted peer records and alias rules (spec §6.5).

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrustedPeer {
    pub endpoint_id: String,
    pub alias: String,
    pub remote_display_name: String,
    pub device_name: String,
    pub paired_at: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AliasError {
    #[error("alias must be 1-32 characters after trimming")]
    InvalidLength,
    #[error("alias must not contain control characters")]
    ContainsControlChars,
    #[error("alias {0:?} is already in use")]
    Duplicate(String),
}

/// Trims and validates a candidate alias against spec §6.5's rules. Does not check uniqueness
/// against existing peers -- callers with a peer list should also call
/// [`find_alias_case_insensitive`] on the trimmed result.
pub fn validate_alias(candidate: &str) -> Result<String, AliasError> {
    let trimmed = candidate.trim();
    let len = trimmed.chars().count();
    if !(1..=32).contains(&len) {
        return Err(AliasError::InvalidLength);
    }
    if trimmed.chars().any(|c| c.is_control()) {
        return Err(AliasError::ContainsControlChars);
    }
    Ok(trimmed.to_string())
}

pub fn find_alias_case_insensitive<'a>(
    peers: &'a [TrustedPeer],
    alias: &str,
) -> Option<&'a TrustedPeer> {
    peers.iter().find(|p| p.alias.eq_ignore_ascii_case(alias))
}

/// Resolves a user-supplied identifier (alias, matched case-insensitively, or an unambiguous
/// `EndpointId` prefix) to exactly one peer, per spec §6.5's ambiguity rule.
pub enum Resolved<'a> {
    Found(&'a TrustedPeer),
    NotFound,
    Ambiguous(Vec<&'a TrustedPeer>),
}

pub fn resolve_identifier<'a>(peers: &'a [TrustedPeer], identifier: &str) -> Resolved<'a> {
    if let Some(peer) = find_alias_case_insensitive(peers, identifier) {
        return Resolved::Found(peer);
    }
    let matches: Vec<&TrustedPeer> = peers
        .iter()
        .filter(|p| p.endpoint_id.starts_with(identifier))
        .collect();
    match matches.len() {
        0 => Resolved::NotFound,
        1 => Resolved::Found(matches[0]),
        _ => Resolved::Ambiguous(matches),
    }
}

/// Picks a unique alias for a newly-paired peer, starting from their display name and
/// appending `-2`, `-3`, ... on collision. Spec §7.5/§7.7 don't define an interactive
/// alias-picking step during pairing, so auto-disambiguating keeps pairing itself simple; a
/// user can still edit `alias` directly in the config file afterward if they want something
/// else, since no `lcp peers rename` command exists in the spec's CLI surface.
pub fn unique_alias(peers: &[TrustedPeer], display_name: &str) -> String {
    let trimmed = display_name.trim();
    let base: String = if trimmed.is_empty() {
        "Peer".to_string()
    } else {
        trimmed.chars().take(28).collect()
    };
    if find_alias_case_insensitive(peers, &base).is_none() {
        return base;
    }
    for n in 2..1000 {
        let candidate = format!("{base}-{n}");
        if find_alias_case_insensitive(peers, &candidate).is_none() {
            return candidate;
        }
    }
    format!("{base}-{}", uuid::Uuid::new_v4().simple())
}

pub fn add_trusted_peer(
    peers: &mut Vec<TrustedPeer>,
    endpoint_id: String,
    display_name: &str,
    device_name: String,
    paired_at: String,
) -> String {
    let alias = unique_alias(peers, display_name);
    peers.push(TrustedPeer {
        endpoint_id,
        alias: alias.clone(),
        remote_display_name: display_name.to_string(),
        device_name,
        paired_at,
    });
    alias
}

pub fn rename_trusted_peer_alias(
    peers: &mut [TrustedPeer],
    endpoint_id: &str,
    new_alias: &str,
) -> Result<String, AliasError> {
    let alias = validate_alias(new_alias)?;
    if peers
        .iter()
        .any(|p| p.endpoint_id != endpoint_id && p.alias.eq_ignore_ascii_case(&alias))
    {
        return Err(AliasError::Duplicate(alias));
    }
    if let Some(peer) = peers.iter_mut().find(|p| p.endpoint_id == endpoint_id) {
        peer.alias = alias.clone();
    }
    Ok(alias)
}

/// Returns `true` if a peer was actually removed.
pub fn remove_trusted_peer(peers: &mut Vec<TrustedPeer>, endpoint_id: &str) -> bool {
    let before = peers.len();
    peers.retain(|p| p.endpoint_id != endpoint_id);
    peers.len() != before
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(alias: &str, endpoint_id: &str) -> TrustedPeer {
        TrustedPeer {
            endpoint_id: endpoint_id.into(),
            alias: alias.into(),
            remote_display_name: alias.into(),
            device_name: "Device".into(),
            paired_at: "2026-08-27T00:00:00Z".into(),
        }
    }

    #[test]
    fn validate_alias_trims_whitespace() {
        assert_eq!(validate_alias("  First  ").unwrap(), "First");
    }

    #[test]
    fn validate_alias_rejects_empty() {
        assert_eq!(
            validate_alias("   ").unwrap_err(),
            AliasError::InvalidLength
        );
    }

    #[test]
    fn validate_alias_rejects_too_long() {
        let long = "a".repeat(33);
        assert_eq!(
            validate_alias(&long).unwrap_err(),
            AliasError::InvalidLength
        );
    }

    #[test]
    fn validate_alias_accepts_max_length() {
        let max = "a".repeat(32);
        assert!(validate_alias(&max).is_ok());
    }

    #[test]
    fn validate_alias_rejects_control_chars() {
        assert_eq!(
            validate_alias("bad\u{0007}name").unwrap_err(),
            AliasError::ContainsControlChars
        );
    }

    #[test]
    fn find_alias_is_case_insensitive() {
        let peers = vec![peer("First", "aaaa1111")];
        assert!(find_alias_case_insensitive(&peers, "first").is_some());
        assert!(find_alias_case_insensitive(&peers, "FIRST").is_some());
        assert!(find_alias_case_insensitive(&peers, "second").is_none());
    }

    #[test]
    fn resolve_identifier_prefers_alias_match() {
        let peers = vec![peer("First", "aaaa1111")];
        assert!(matches!(
            resolve_identifier(&peers, "First"),
            Resolved::Found(p) if p.endpoint_id == "aaaa1111"
        ));
    }

    #[test]
    fn resolve_identifier_falls_back_to_unambiguous_prefix() {
        let peers = vec![peer("First", "aaaa1111"), peer("Second", "bbbb2222")];
        assert!(matches!(
            resolve_identifier(&peers, "aaaa"),
            Resolved::Found(p) if p.alias == "First"
        ));
    }

    #[test]
    fn resolve_identifier_reports_ambiguous_prefix() {
        let peers = vec![peer("First", "aaaa1111"), peer("Second", "aaaa2222")];
        assert!(matches!(
            resolve_identifier(&peers, "aaaa"),
            Resolved::Ambiguous(matches) if matches.len() == 2
        ));
    }

    #[test]
    fn resolve_identifier_reports_not_found() {
        let peers = vec![peer("First", "aaaa1111")];
        assert!(matches!(
            resolve_identifier(&peers, "zzzz"),
            Resolved::NotFound
        ));
    }

    #[test]
    fn unique_alias_uses_display_name_when_free() {
        let peers = vec![];
        assert_eq!(unique_alias(&peers, "First"), "First");
    }

    #[test]
    fn unique_alias_disambiguates_on_collision() {
        let peers = vec![peer("First", "aaaa1111")];
        assert_eq!(unique_alias(&peers, "First"), "First-2");
    }

    #[test]
    fn add_and_remove_trusted_peer_round_trip() {
        let mut peers = vec![];
        let alias = add_trusted_peer(
            &mut peers,
            "endpoint-a".into(),
            "First",
            "First-PC".into(),
            "2026-08-27T00:00:00Z".into(),
        );
        assert_eq!(alias, "First");
        assert_eq!(peers.len(), 1);
        assert!(remove_trusted_peer(&mut peers, "endpoint-a"));
        assert!(peers.is_empty());
        assert!(!remove_trusted_peer(&mut peers, "endpoint-a"));
    }

    #[test]
    fn rename_trusted_peer_alias_updates_alias() {
        let mut peers = vec![peer("First", "aaaa1111")];
        let alias = rename_trusted_peer_alias(&mut peers, "aaaa1111", "Laptop").unwrap();
        assert_eq!(alias, "Laptop");
        assert_eq!(peers[0].alias, "Laptop");
    }

    #[test]
    fn rename_trusted_peer_alias_rejects_duplicate() {
        let mut peers = vec![peer("First", "aaaa1111"), peer("Second", "bbbb2222")];
        assert_eq!(
            rename_trusted_peer_alias(&mut peers, "aaaa1111", "second").unwrap_err(),
            AliasError::Duplicate("second".into())
        );
    }
}
