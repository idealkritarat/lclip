use lcp_protocol::ipc::methods;
use lcp_protocol::IPC_PROTOCOL_VERSION;

use crate::{clipboard, daemon_conn, output, unwrap_response};

pub async fn run(peer: Option<&str>) -> anyhow::Result<i32> {
    let client = daemon_conn::connect_or_autostart().await?;
    let params = match peer {
        Some(p) => serde_json::json!({"peer": p}),
        None => serde_json::json!({}),
    };
    let resp = client
        .call(IPC_PROTOCOL_VERSION, methods::GET_LATEST_INCOMING, params)
        .await?;
    match unwrap_response(resp) {
        Ok(value) => {
            let text = value.get("text").and_then(|v| v.as_str()).unwrap_or("");
            match clipboard::write_text(text) {
                Ok(()) => {
                    match peer {
                        Some(p) => println!("Copied latest message from {p}."),
                        None => println!("Copied latest message."),
                    }
                    Ok(output::exit_code::SUCCESS)
                }
                Err(e) => {
                    eprintln!("Error: could not write clipboard: {e}");
                    Ok(output::exit_code::GENERAL_ERROR)
                }
            }
        }
        Err(err) => {
            eprintln!("Error: {}", err.message);
            Ok(crate::exit_code_for(&err))
        }
    }
}
