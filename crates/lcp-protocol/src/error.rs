use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("frame is malformed")]
    MalformedFrame,
    #[error("frame of {actual} bytes exceeds the {max}-byte limit")]
    FrameTooLarge { max: usize, actual: usize },
    #[error("frame length prefix was zero")]
    EmptyFrame,
    #[error("unsupported protocol version {found} (expected {expected})")]
    UnsupportedVersion { found: u16, expected: u16 },
    #[error("invalid ticket: {0}")]
    InvalidTicket(String),
    #[error("ticket has expired")]
    TicketExpired,
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("text payload of {actual} bytes exceeds the {max}-byte limit")]
    TextTooLarge { max: usize, actual: usize },
}
