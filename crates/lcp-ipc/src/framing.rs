use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use lcp_protocol::ipc::{decode_length_prefix, IPC_FRAME_LENGTH_PREFIX_BYTES, MAX_IPC_FRAME_BYTES};
use lcp_protocol::ProtocolError;

#[derive(Debug, thiserror::Error)]
pub enum FramingError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    #[error("connection closed")]
    Closed,
}

pub async fn write_frame<W: AsyncWrite + Unpin>(
    writer: &mut W,
    payload: &[u8],
) -> Result<(), FramingError> {
    if payload.len() > MAX_IPC_FRAME_BYTES {
        return Err(FramingError::Protocol(ProtocolError::FrameTooLarge {
            max: MAX_IPC_FRAME_BYTES,
            actual: payload.len(),
        }));
    }
    let len = (payload.len() as u32).to_be_bytes();
    writer.write_all(&len).await?;
    writer.write_all(payload).await?;
    writer.flush().await?;
    Ok(())
}

pub async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Vec<u8>, FramingError> {
    let mut header = [0u8; IPC_FRAME_LENGTH_PREFIX_BYTES];
    match reader.read_exact(&mut header).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
            return Err(FramingError::Closed)
        }
        Err(e) => return Err(e.into()),
    }
    let len = decode_length_prefix(header, MAX_IPC_FRAME_BYTES)?;
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;
    Ok(buf)
}
