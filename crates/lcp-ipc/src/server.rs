use std::future::Future;
use std::sync::Arc;

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;

use lcp_protocol::ipc::{IpcEvent, IpcRequest, IpcResponse};

use crate::framing::{read_frame, write_frame, FramingError};

pub type EventSender = mpsc::UnboundedSender<IpcEvent>;

/// Implemented once by `lanclipd` and shared (via `Arc`) across every accepted connection.
/// `lcp-ipc` never depends on `lcp-core` -- this trait is the seam where the daemon's actual
/// method dispatch gets plugged into the transport-agnostic connection loop below.
pub trait RequestHandler: Send + Sync + 'static {
    fn handle(
        &self,
        request: IpcRequest,
        events: EventSender,
    ) -> impl Future<Output = IpcResponse> + Send;
}

/// Drives one accepted connection until it closes: reads framed requests, dispatches each to
/// `handler`, writes back the framed response, and interleaves any events the handler later
/// pushes onto the per-connection `EventSender` (used by `subscribe`).
pub async fn handle_connection<S, H>(stream: S, handler: Arc<H>)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    H: RequestHandler,
{
    let (mut reader, writer) = tokio::io::split(stream);
    let writer = Arc::new(tokio::sync::Mutex::new(writer));
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<IpcEvent>();

    let event_writer = writer.clone();
    let event_task = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            let Ok(bytes) = serde_json::to_vec(&event) else {
                continue;
            };
            let mut w = event_writer.lock().await;
            if write_frame(&mut *w, &bytes).await.is_err() {
                break;
            }
        }
    });

    loop {
        let bytes = match read_frame(&mut reader).await {
            Ok(bytes) => bytes,
            Err(FramingError::Closed) => break,
            Err(e) => {
                tracing::debug!(error = %e, "IPC read error, closing connection");
                break;
            }
        };
        let request: IpcRequest = match serde_json::from_slice(&bytes) {
            Ok(r) => r,
            Err(e) => {
                // Never log serde_json's rendered error: it can echo a fragment of the
                // offending JSON, which for a request may be clipboard/message text (spec
                // §14.6). `classify()` is a category enum with no payload content.
                tracing::debug!(error_kind = ?e.classify(), "malformed IPC request, closing connection");
                break;
            }
        };
        let response = handler.handle(request, event_tx.clone()).await;
        let Ok(resp_bytes) = serde_json::to_vec(&response) else {
            break;
        };
        let mut w = writer.lock().await;
        let write_result = write_frame(&mut *w, &resp_bytes).await;
        drop(w);
        if write_result.is_err() {
            break;
        }
    }

    event_task.abort();
}
