//! Pure wire, config, and IPC types shared across LCP crates. No I/O lives here.

pub mod error;
pub mod ipc;
pub mod network;
pub mod ticket;

pub use error::ProtocolError;

/// Iroh ALPN identifying the LCP network protocol.
pub const ALPN: &[u8] = b"lcp/1";

/// Current network wire protocol version. A mismatch must fail clearly, never downgrade silently.
pub const NETWORK_PROTOCOL_VERSION: u16 = 1;

/// Current local IPC protocol version.
pub const IPC_PROTOCOL_VERSION: u16 = 1;

/// Current on-disk config schema version.
pub const CONFIG_SCHEMA_VERSION: u32 = 1;
