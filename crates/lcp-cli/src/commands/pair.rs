use lcp_protocol::ipc::{events, methods};
use lcp_protocol::IPC_PROTOCOL_VERSION;

use crate::{daemon_conn, output, unwrap_response};

pub async fn run(ticket: &str) -> anyhow::Result<i32> {
    let client = daemon_conn::connect_or_autostart().await?;
    let _ = client
        .call(
            IPC_PROTOCOL_VERSION,
            methods::SUBSCRIBE,
            serde_json::json!({}),
        )
        .await?;

    let resp = client
        .call(
            IPC_PROTOCOL_VERSION,
            methods::JOIN_INVITE,
            serde_json::json!({"ticket": ticket}),
        )
        .await?;
    let value = match unwrap_response(resp) {
        Ok(v) => v,
        Err(err) => {
            eprintln!("Error: {}", err.message);
            return Ok(crate::exit_code_for(&err));
        }
    };
    let pairing_id = value
        .get("pairing_id")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let verification = value
        .get("verification_string")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    println!("Verification string: {verification}");
    let confirmed =
        output::prompt_yes_no("Does this match on their screen too? Confirm pairing? [y/N] ");
    let method = if confirmed {
        methods::CONFIRM_PAIRING
    } else {
        methods::REJECT_PAIRING
    };
    let resp = client
        .call(
            IPC_PROTOCOL_VERSION,
            method,
            serde_json::json!({"pairing_id": pairing_id}),
        )
        .await?;
    if let Err(err) = unwrap_response(resp) {
        eprintln!("Error: {}", err.message);
        return Ok(crate::exit_code_for(&err));
    }
    if !confirmed {
        println!("Cancelled.");
        return Ok(output::exit_code::SUCCESS);
    }

    loop {
        let event = match client.next_event().await {
            Some(e) => e,
            None => {
                eprintln!("Error: lost connection to the daemon while waiting.");
                return Ok(output::exit_code::DAEMON_UNAVAILABLE);
            }
        };
        if event.event != events::PAIRING_UPDATED {
            continue;
        }
        if event.payload.get("pairing_id").and_then(|v| v.as_str()) != Some(pairing_id.as_str()) {
            continue;
        }
        let status = event
            .payload
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        return Ok(match status {
            "paired" => {
                let alias = event
                    .payload
                    .get("alias")
                    .and_then(|v| v.as_str())
                    .unwrap_or("peer");
                println!("Paired with {alias}.");
                output::exit_code::SUCCESS
            }
            "rejected" => {
                println!("The other side rejected pairing.");
                output::exit_code::PAIRING_FAILURE
            }
            _ => {
                let reason = event
                    .payload
                    .get("reason")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown error");
                println!("Pairing failed: {reason}");
                output::exit_code::PAIRING_FAILURE
            }
        });
    }
}
