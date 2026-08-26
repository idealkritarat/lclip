use lcp_protocol::ipc::methods;
use lcp_protocol::IPC_PROTOCOL_VERSION;

use crate::{daemon_conn, output, unwrap_response};

pub async fn run(peer: &str, yes: bool) -> anyhow::Result<i32> {
    if !yes {
        let confirmed = output::prompt_yes_no(&format!(
            "Unpair {peer}? This cannot be undone without pairing again. [y/N] "
        ));
        if !confirmed {
            println!("Cancelled.");
            return Ok(output::exit_code::SUCCESS);
        }
    }

    let client = daemon_conn::connect_or_autostart().await?;
    let resp = client
        .call(
            IPC_PROTOCOL_VERSION,
            methods::UNPAIR_PEER,
            serde_json::json!({"peer": peer}),
        )
        .await?;
    match unwrap_response(resp) {
        Ok(_) => {
            println!("Unpaired {peer}.");
            Ok(output::exit_code::SUCCESS)
        }
        Err(err) => {
            eprintln!("Error: {}", err.message);
            Ok(crate::exit_code_for(&err))
        }
    }
}
