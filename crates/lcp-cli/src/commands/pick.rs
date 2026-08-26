use std::io::IsTerminal;

use lcp_protocol::ipc::methods;
use lcp_protocol::IPC_PROTOCOL_VERSION;

use crate::picker::{self, PickOutcome, PickerRow};
use crate::{clipboard, daemon_conn, output, unwrap_response};

pub async fn run(peer: Option<&str>, incoming_only: bool) -> anyhow::Result<i32> {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        eprintln!("Error: `lcp pick` requires an interactive terminal.");
        return Ok(output::exit_code::INVALID_ARGS);
    }

    let client = daemon_conn::connect_or_autostart().await?;
    let params = serde_json::json!({"peer": peer, "incoming_only": incoming_only});
    let resp = client
        .call(IPC_PROTOCOL_VERSION, methods::LIST_MESSAGES, params)
        .await?;
    let messages = match unwrap_response(resp) {
        Ok(value) => value.as_array().cloned().unwrap_or_default(),
        Err(err) => {
            eprintln!("Error: {}", err.message);
            return Ok(crate::exit_code_for(&err));
        }
    };

    if messages.is_empty() {
        println!("No messages to show yet.");
        return Ok(output::exit_code::SUCCESS);
    }

    let rows: Vec<PickerRow> = messages
        .iter()
        .map(|m| PickerRow {
            sender_label: m
                .get("sender_label")
                .and_then(|v| v.as_str())
                .unwrap_or("?")
                .to_string(),
            received_at_unix_ms: m
                .get("received_at_unix_ms")
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
            text: m
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        })
        .collect();

    let title = match peer {
        Some(p) => format!("Choose a message to copy \u{2014} {p}"),
        None => "Choose a message to copy".to_string(),
    };

    let outcome = tokio::task::spawn_blocking(move || picker::run(&title, &rows)).await??;
    match outcome {
        PickOutcome::Selected(text) => match clipboard::write_text(&text) {
            Ok(()) => {
                println!("Copied.");
                Ok(output::exit_code::SUCCESS)
            }
            Err(e) => {
                eprintln!("Error: could not write clipboard: {e}");
                Ok(output::exit_code::GENERAL_ERROR)
            }
        },
        PickOutcome::Cancelled => Ok(output::exit_code::SUCCESS),
        PickOutcome::Interrupted => Ok(output::exit_code::INTERRUPTED),
    }
}
