use lcp_protocol::ipc::{events, methods};
use lcp_protocol::IPC_PROTOCOL_VERSION;

use crate::{clipboard, daemon_conn, output, unwrap_response};

pub async fn run(ttl_secs: u64, no_copy: bool) -> anyhow::Result<i32> {
    let client = daemon_conn::connect_or_autostart().await?;
    // Subscribe before creating the invite so we can't miss a fast pairing_requested event.
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
            methods::CREATE_INVITE,
            serde_json::json!({"ttl_secs": ttl_secs}),
        )
        .await?;
    let value = match unwrap_response(resp) {
        Ok(v) => v,
        Err(err) => {
            eprintln!("Error: {}", err.message);
            return Ok(crate::exit_code_for(&err));
        }
    };
    let ticket = value
        .get("ticket")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let ttl = value
        .get("ttl_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(ttl_secs);

    if no_copy {
        println!("{ticket}\n");
    } else {
        match clipboard::write_text(ticket) {
            Ok(()) => println!("Pairing ticket copied to clipboard.\n\n{ticket}\n"),
            Err(_) => println!("(could not copy ticket to clipboard)\n\n{ticket}\n"),
        }
    }
    println!(
        "Waiting for a peer for {} minutes. Press Ctrl+C to cancel.",
        ttl.div_ceil(60)
    );

    loop {
        let event = match client.next_event().await {
            Some(e) => e,
            None => {
                eprintln!("Error: lost connection to the daemon while waiting.");
                return Ok(output::exit_code::DAEMON_UNAVAILABLE);
            }
        };
        match event.event.as_str() {
            events::PAIRING_REQUESTED => {
                let pairing_id = event
                    .payload
                    .get("pairing_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let peer_name = event
                    .payload
                    .get("peer_display_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("someone");
                let peer_device = event
                    .payload
                    .get("peer_device_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("a device");
                let verification = event
                    .payload
                    .get("verification_string")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                println!("\n{peer_name} ({peer_device}) wants to pair.");
                println!("Verification string: {verification}");
                let confirmed = output::prompt_yes_no(
                    "Does this match on their screen too? Confirm pairing? [y/N] ",
                );
                let method = if confirmed {
                    methods::CONFIRM_PAIRING
                } else {
                    methods::REJECT_PAIRING
                };
                let _ = client
                    .call(
                        IPC_PROTOCOL_VERSION,
                        method,
                        serde_json::json!({"pairing_id": pairing_id}),
                    )
                    .await;
            }
            events::PAIRING_UPDATED => {
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
                        println!("Pairing was rejected.");
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
            _ => {}
        }
    }
}
