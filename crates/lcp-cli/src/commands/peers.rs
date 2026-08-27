use lcp_protocol::ipc::methods;
use lcp_protocol::IPC_PROTOCOL_VERSION;

use crate::{daemon_conn, output, unwrap_response};

/// Left-pads to `width`, but always leaves at least a 2-space gap even when `value` itself is
/// longer than `width` -- a plain `{:<width$}` silently glues columns together in that case.
fn pad(value: &str, width: usize) -> String {
    let width = width.max(value.chars().count() + 2);
    format!("{value:<width$}")
}

pub async fn run(json: bool) -> anyhow::Result<i32> {
    let client = daemon_conn::connect_or_autostart().await?;
    let resp = client
        .call(
            IPC_PROTOCOL_VERSION,
            methods::LIST_PEERS,
            serde_json::json!({}),
        )
        .await?;
    match unwrap_response(resp) {
        Ok(value) => {
            if json {
                output::print_json(&value);
                return Ok(output::exit_code::SUCCESS);
            }
            let peers = value.as_array().cloned().unwrap_or_default();
            if peers.is_empty() {
                println!("No paired peers yet. Run `lcp invite` on this machine, or `lcp pair <ticket>` with one a friend sent you.");
                return Ok(output::exit_code::SUCCESS);
            }
            println!(
                "{}{}{}PATH",
                pad("NAME", 10),
                pad("DEVICE", 13),
                pad("STATUS", 12)
            );
            for peer in peers {
                let field = |name: &str| {
                    peer.get(name)
                        .and_then(|v| v.as_str())
                        .unwrap_or("-")
                        .to_string()
                };
                let path = field("path");
                let path = if path == "unknown" {
                    "-".to_string()
                } else {
                    path
                };
                println!(
                    "{}{}{}{}",
                    pad(&field("alias"), 10),
                    pad(&field("device_name"), 13),
                    pad(&field("status"), 12),
                    path
                );
            }
            Ok(output::exit_code::SUCCESS)
        }
        Err(err) => {
            eprintln!("Error: {}", err.message);
            Ok(output::exit_code::GENERAL_ERROR)
        }
    }
}

pub async fn rename(peer: &str, alias: &str) -> anyhow::Result<i32> {
    let client = daemon_conn::connect_or_autostart().await?;
    let resp = client
        .call(
            IPC_PROTOCOL_VERSION,
            methods::RENAME_PEER,
            serde_json::json!({"peer": peer, "alias": alias}),
        )
        .await?;
    match unwrap_response(resp) {
        Ok(value) => {
            let alias = value.get("alias").and_then(|v| v.as_str()).unwrap_or(alias);
            println!("Renamed peer to {alias}.");
            Ok(output::exit_code::SUCCESS)
        }
        Err(err) => {
            eprintln!("Error: {}", err.message);
            Ok(crate::exit_code_for(&err))
        }
    }
}
