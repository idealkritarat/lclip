use lcp_protocol::ipc::methods;
use lcp_protocol::IPC_PROTOCOL_VERSION;

use crate::{daemon_conn, output, unwrap_response};

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
            println!("{:<10}{:<13}{:<12}PATH", "NAME", "DEVICE", "STATUS");
            for peer in peers {
                let field = |name: &str| {
                    peer.get(name)
                        .and_then(|v| v.as_str())
                        .unwrap_or("-")
                        .to_string()
                };
                println!(
                    "{:<10}{:<13}{:<12}{}",
                    field("alias"),
                    field("device_name"),
                    field("status"),
                    field("path")
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
