use std::io::Write;

use lcp_protocol::ipc::methods;
use lcp_protocol::IPC_PROTOCOL_VERSION;

use crate::{daemon_conn, output, unwrap_response};

pub async fn run(peer: Option<&str>, json: bool) -> anyhow::Result<i32> {
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
            if json {
                output::print_json(&value);
                return Ok(output::exit_code::SUCCESS);
            }
            // Raw mode: exact bytes, no label, no trailing newline that wasn't already there.
            let text = value.get("text").and_then(|v| v.as_str()).unwrap_or("");
            std::io::stdout().write_all(text.as_bytes())?;
            Ok(output::exit_code::SUCCESS)
        }
        Err(err) => {
            eprintln!("Error: {}", err.message);
            Ok(crate::exit_code_for(&err))
        }
    }
}
