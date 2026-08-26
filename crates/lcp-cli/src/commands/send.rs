use std::io::Read;

use lcp_protocol::ipc::methods;
use lcp_protocol::network::MAX_TEXT_BYTES;
use lcp_protocol::IPC_PROTOCOL_VERSION;

use crate::{clipboard, daemon_conn, output, unwrap_response};

pub async fn run(peer: &str, use_stdin: bool, text_arg: Option<&str>) -> anyhow::Result<i32> {
    let text = match (use_stdin, text_arg) {
        (true, _) => match read_stdin_capped() {
            Ok(text) => text,
            Err(message) => {
                eprintln!("Error: {message}");
                return Ok(output::exit_code::INVALID_ARGS);
            }
        },
        (false, Some(text)) => text.to_string(),
        (false, None) => match clipboard::read_text() {
            Ok(text) => text,
            Err(e) => {
                eprintln!("Error: could not read clipboard: {e}");
                return Ok(output::exit_code::INVALID_ARGS);
            }
        },
    };

    if text.is_empty() {
        eprintln!("Error: refusing to send empty text.");
        return Ok(output::exit_code::INVALID_ARGS);
    }
    if text.len() > MAX_TEXT_BYTES {
        eprintln!(
            "Error: text is {} bytes, over the {MAX_TEXT_BYTES}-byte limit.",
            text.len()
        );
        return Ok(output::exit_code::LIMIT_EXCEEDED);
    }

    let client = daemon_conn::connect_or_autostart().await?;
    let resp = client
        .call(
            IPC_PROTOCOL_VERSION,
            methods::SEND_TEXT,
            serde_json::json!({"peer": peer, "text": text}),
        )
        .await?;
    match unwrap_response(resp) {
        Ok(_) => {
            println!("Sent to {peer}.");
            Ok(output::exit_code::SUCCESS)
        }
        Err(err) => {
            eprintln!("Error: {}", err.message);
            Ok(crate::exit_code_for(&err))
        }
    }
}

/// Reads stdin to EOF, capped at [`MAX_TEXT_BYTES`] + 1 so an oversized stream is detected
/// without buffering an unbounded amount of attacker/user-supplied input first.
fn read_stdin_capped() -> Result<String, String> {
    let mut buf = Vec::new();
    std::io::stdin()
        .take(MAX_TEXT_BYTES as u64 + 1)
        .read_to_end(&mut buf)
        .map_err(|e| format!("could not read stdin: {e}"))?;
    if buf.len() > MAX_TEXT_BYTES {
        return Err(format!("stdin exceeds the {MAX_TEXT_BYTES}-byte limit"));
    }
    String::from_utf8(buf).map_err(|_| "stdin is not valid UTF-8".to_string())
}
