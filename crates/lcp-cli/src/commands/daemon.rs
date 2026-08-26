use std::time::Duration;

use lcp_protocol::ipc::methods;
use lcp_protocol::IPC_PROTOCOL_VERSION;

use crate::{autostart, daemon_conn, output, unwrap_response};

pub async fn status() -> anyhow::Result<i32> {
    match daemon_conn::try_connect().await {
        Ok(client) => {
            let resp = client
                .call(IPC_PROTOCOL_VERSION, methods::HELLO, serde_json::json!({}))
                .await?;
            match unwrap_response(resp) {
                Ok(_) => {
                    println!("lanclipd is running.");
                    Ok(output::exit_code::SUCCESS)
                }
                Err(err) => {
                    println!(
                        "lanclipd is reachable but not responding correctly: {}",
                        err.message
                    );
                    Ok(output::exit_code::DAEMON_UNAVAILABLE)
                }
            }
        }
        Err(_) => {
            println!("lanclipd is not running.");
            Ok(output::exit_code::SUCCESS)
        }
    }
}

/// Idempotent: if the daemon is already running, this just reports that (spec §11.12).
pub async fn start() -> anyhow::Result<i32> {
    if daemon_conn::try_connect().await.is_ok() {
        println!("lanclipd is already running.");
        return Ok(output::exit_code::SUCCESS);
    }
    daemon_conn::spawn_daemon()?;
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_millis(150)).await;
        if daemon_conn::try_connect().await.is_ok() {
            println!("lanclipd started.");
            return Ok(output::exit_code::SUCCESS);
        }
    }
    eprintln!("Error: started lanclipd but it never became reachable over IPC.");
    Ok(output::exit_code::DAEMON_UNAVAILABLE)
}

pub async fn stop() -> anyhow::Result<i32> {
    match daemon_conn::try_connect().await {
        Ok(client) => {
            let _ = client
                .call(
                    IPC_PROTOCOL_VERSION,
                    methods::SHUTDOWN,
                    serde_json::json!({}),
                )
                .await;
            if daemon_conn::wait_until_unreachable(Duration::from_secs(5)).await {
                println!("lanclipd stopped.");
                Ok(output::exit_code::SUCCESS)
            } else {
                eprintln!("Error: lanclipd did not stop within 5 seconds.");
                Ok(output::exit_code::GENERAL_ERROR)
            }
        }
        Err(_) => {
            println!("lanclipd was not running.");
            Ok(output::exit_code::SUCCESS)
        }
    }
}

/// Waits for the old IPC endpoint to actually close before spawning a new instance (spec §11.12).
pub async fn restart() -> anyhow::Result<i32> {
    if daemon_conn::try_connect().await.is_ok() {
        let code = stop().await?;
        if code != output::exit_code::SUCCESS {
            return Ok(code);
        }
    }
    start().await
}

pub async fn install() -> anyhow::Result<i32> {
    autostart::install()?;
    println!("Autostart installed for the current user.");
    Ok(output::exit_code::SUCCESS)
}

pub async fn uninstall() -> anyhow::Result<i32> {
    autostart::uninstall()?;
    println!("Autostart removed. Pairing and identity are unaffected.");
    Ok(output::exit_code::SUCCESS)
}
