//! Cross-platform local IPC transport: Unix domain socket on macOS, named pipe on Windows.
//! Both sides speak the same 4-byte-length-prefixed JSON framing over whatever the platform
//! module hands back, since both stream types implement `AsyncRead + AsyncWrite`.

mod framing;

pub mod client;
pub mod server;

#[cfg(unix)]
pub mod unix;

#[cfg(windows)]
pub mod windows;

pub use framing::{read_frame, write_frame, FramingError};
pub use server::{handle_connection, EventSender, RequestHandler};
