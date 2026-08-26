use lcp_protocol::ipc::methods;
use lcp_protocol::IPC_PROTOCOL_VERSION;

use crate::{daemon_conn, output, unwrap_response};

pub async fn run(json: bool) -> anyhow::Result<i32> {
    let client = match daemon_conn::connect_or_autostart().await {
        Ok(client) => client,
        Err(e) => {
            eprintln!("Error: could not reach or start lanclipd: {e}");
            return Ok(output::exit_code::DAEMON_UNAVAILABLE);
        }
    };
    let resp = client
        .call(
            IPC_PROTOCOL_VERSION,
            methods::GET_STATUS,
            serde_json::json!({}),
        )
        .await?;
    let value = match unwrap_response(resp) {
        Ok(value) => value,
        Err(message) => {
            eprintln!("Error: {message}");
            return Ok(output::exit_code::GENERAL_ERROR);
        }
    };

    if json {
        output::print_json(&value);
        return Ok(output::exit_code::SUCCESS);
    }

    let field = |name: &str| {
        value
            .get(name)
            .and_then(|v| v.as_str())
            .unwrap_or("-")
            .to_string()
    };
    let count = |name: &str| value.get(name).and_then(|v| v.as_u64()).unwrap_or(0);

    println!("Daemon       running");
    println!("Identity     {}", field("endpoint_id_prefix"));
    println!("Relay        {}", field("relay_mode"));
    println!(
        "Peers        {} online / {} paired",
        count("online_peer_count"),
        count("trusted_peer_count")
    );
    println!("History      memory only");
    println!("Uptime       {}", format_uptime(count("uptime_secs")));
    Ok(output::exit_code::SUCCESS)
}

fn format_uptime(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}
