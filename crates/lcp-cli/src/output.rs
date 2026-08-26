//! Exit codes (spec §11.13) and small human/JSON output helpers.

/// The full spec §11.13 table, defined up front since it's a fixed contract other tooling can
/// rely on. Variants get used as their commands land phase by phase, so this stays
/// `allow(dead_code)` rather than growing piecemeal.
#[allow(dead_code)]
pub mod exit_code {
    pub const SUCCESS: i32 = 0;
    pub const GENERAL_ERROR: i32 = 1;
    pub const INVALID_ARGS: i32 = 2;
    pub const PEER_NOT_FOUND: i32 = 3;
    pub const PEER_OFFLINE: i32 = 4;
    pub const NO_MESSAGE: i32 = 5;
    pub const DAEMON_UNAVAILABLE: i32 = 6;
    pub const PAIRING_FAILURE: i32 = 7;
    pub const LIMIT_EXCEEDED: i32 = 8;
    pub const VERSION_MISMATCH: i32 = 9;
    pub const PERMISSION_FAILURE: i32 = 10;
    pub const INTERRUPTED: i32 = 130;
}

pub fn print_json(value: &serde_json::Value) {
    match serde_json::to_string_pretty(value) {
        Ok(text) => println!("{text}"),
        Err(_) => println!("{value}"),
    }
}
