use lcp_protocol::ipc::methods;
use lcp_protocol::IPC_PROTOCOL_VERSION;

use crate::{daemon_conn, output, unwrap_response};

/// Runs client-side checks (daemon reachability) plus, if reachable, the daemon's own
/// self-checks (spec §11.11, §17.2). Never auto-starts the daemon -- doctor should report
/// what's actually true right now, not change state first.
pub async fn run(json: bool) -> anyhow::Result<i32> {
    let mut checks: Vec<serde_json::Value> = Vec::new();
    let daemon_unreachable;

    match daemon_conn::try_connect().await {
        Ok(client) => {
            daemon_unreachable = false;
            checks.push(serde_json::json!({
                "id": "daemon_running",
                "severity": "ok",
                "summary": "lanclipd is running",
                "detail": "",
                "suggested_action": null,
            }));

            match client
                .call(
                    IPC_PROTOCOL_VERSION,
                    methods::RUN_DIAGNOSTICS,
                    serde_json::json!({}),
                )
                .await
            {
                Ok(resp) => match unwrap_response(resp) {
                    Ok(value) => {
                        if let Some(arr) = value.as_array() {
                            checks.extend(arr.iter().cloned());
                        }
                    }
                    Err(err) => checks.push(serde_json::json!({
                        "id": "run_diagnostics",
                        "severity": "error",
                        "summary": "Could not run daemon diagnostics",
                        "detail": err.message,
                        "suggested_action": null,
                    })),
                },
                Err(e) => checks.push(serde_json::json!({
                    "id": "run_diagnostics",
                    "severity": "error",
                    "summary": "Lost connection to daemon while running diagnostics",
                    "detail": e.to_string(),
                    "suggested_action": null,
                })),
            }
        }
        Err(e) => {
            daemon_unreachable = true;
            checks.push(serde_json::json!({
                "id": "daemon_running",
                "severity": "error",
                "summary": "lanclipd is not reachable",
                "detail": e.to_string(),
                "suggested_action": "Run `lcp daemon start`.",
            }));
        }
    }

    let has_error = checks
        .iter()
        .any(|c| c.get("severity").and_then(|v| v.as_str()) == Some("error"));

    if json {
        output::print_json(&serde_json::Value::Array(checks));
    } else {
        for check in &checks {
            let severity = check
                .get("severity")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let marker = match severity {
                "ok" => "[ok]   ",
                "warning" => "[warn] ",
                "error" => "[error]",
                _ => "[?]    ",
            };
            let summary = check.get("summary").and_then(|v| v.as_str()).unwrap_or("");
            println!("{marker} {summary}");
            if let Some(detail) = check.get("detail").and_then(|v| v.as_str()) {
                if !detail.is_empty() {
                    println!("          {detail}");
                }
            }
            if let Some(action) = check.get("suggested_action").and_then(|v| v.as_str()) {
                println!("          Suggested: {action}");
            }
        }
    }

    Ok(if daemon_unreachable {
        output::exit_code::DAEMON_UNAVAILABLE
    } else if has_error {
        output::exit_code::GENERAL_ERROR
    } else {
        output::exit_code::SUCCESS
    })
}
