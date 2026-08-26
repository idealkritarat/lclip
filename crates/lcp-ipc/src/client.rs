use std::collections::HashMap;
use std::sync::Arc;

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{mpsc, oneshot, Mutex};
use uuid::Uuid;

use lcp_protocol::ipc::{IpcEvent, IpcRequest, IpcResponse, IpcServerFrame};

use crate::framing::{read_frame, write_frame};

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("daemon closed the connection")]
    Disconnected,
}

/// A connected IPC client. Constructed once per platform-specific `connect()` via [`Self::spawn`],
/// after which it is a concrete, platform-erased handle usable uniformly by `lcp-cli`.
pub struct IpcClient {
    request_tx: mpsc::UnboundedSender<(IpcRequest, oneshot::Sender<IpcResponse>)>,
    event_rx: Mutex<mpsc::UnboundedReceiver<IpcEvent>>,
}

impl IpcClient {
    pub fn spawn<S>(stream: S) -> Self
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let (request_tx, request_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        tokio::spawn(run(stream, request_rx, event_tx));
        Self {
            request_tx,
            event_rx: Mutex::new(event_rx),
        }
    }

    pub async fn call(
        &self,
        ipc_version: u16,
        method: impl Into<String>,
        params: serde_json::Value,
    ) -> Result<IpcResponse, ClientError> {
        let request = IpcRequest::new(ipc_version, method, params);
        let (tx, rx) = oneshot::channel();
        self.request_tx
            .send((request, tx))
            .map_err(|_| ClientError::Disconnected)?;
        rx.await.map_err(|_| ClientError::Disconnected)
    }

    /// Awaits the next unsolicited event. Only meaningful after issuing a `subscribe` call.
    pub async fn next_event(&self) -> Option<IpcEvent> {
        self.event_rx.lock().await.recv().await
    }
}

async fn run<S>(
    stream: S,
    mut request_rx: mpsc::UnboundedReceiver<(IpcRequest, oneshot::Sender<IpcResponse>)>,
    event_tx: mpsc::UnboundedSender<IpcEvent>,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut reader, mut writer) = tokio::io::split(stream);
    let pending: Arc<Mutex<HashMap<Uuid, oneshot::Sender<IpcResponse>>>> =
        Arc::new(Mutex::new(HashMap::new()));

    let pending_for_reader = pending.clone();
    let read_task = tokio::spawn(async move {
        loop {
            let bytes = match read_frame(&mut reader).await {
                Ok(bytes) => bytes,
                Err(_) => break,
            };
            match serde_json::from_slice::<IpcServerFrame>(&bytes) {
                Ok(IpcServerFrame::Response(resp)) => {
                    if let Some(tx) = pending_for_reader.lock().await.remove(&resp.id) {
                        let _ = tx.send(resp);
                    }
                }
                Ok(IpcServerFrame::Event(event)) => {
                    let _ = event_tx.send(event);
                }
                Err(e) => {
                    tracing::debug!(error = %e, "malformed frame from daemon, ignoring");
                }
            }
        }
    });

    while let Some((request, respond_to)) = request_rx.recv().await {
        pending.lock().await.insert(request.id, respond_to);
        let Ok(bytes) = serde_json::to_vec(&request) else {
            continue;
        };
        if write_frame(&mut writer, &bytes).await.is_err() {
            break;
        }
    }
    read_task.abort();
}
