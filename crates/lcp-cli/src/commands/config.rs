use lcp_protocol::ipc::methods;
use lcp_protocol::IPC_PROTOCOL_VERSION;

use crate::{daemon_conn, output, unwrap_response};

pub async fn get(key: &str, json: bool) -> anyhow::Result<i32> {
    let client = daemon_conn::connect_or_autostart().await?;
    let resp = client
        .call(
            IPC_PROTOCOL_VERSION,
            methods::GET_CONFIG,
            serde_json::json!({"key": key}),
        )
        .await?;
    match unwrap_response(resp) {
        Ok(value) => {
            if json {
                output::print_json(&value);
            } else {
                println!("{}", scalar_to_string(&value));
            }
            Ok(output::exit_code::SUCCESS)
        }
        Err(err) => {
            eprintln!("Error: {}", err.message);
            Ok(output::exit_code::INVALID_ARGS)
        }
    }
}

pub async fn set(key: &str, value: &str) -> anyhow::Result<i32> {
    let client = daemon_conn::connect_or_autostart().await?;
    let resp = client
        .call(
            IPC_PROTOCOL_VERSION,
            methods::SET_CONFIG,
            serde_json::json!({"key": key, "value": value}),
        )
        .await?;
    match unwrap_response(resp) {
        Ok(_) => {
            println!("{key} = {value}");
            Ok(output::exit_code::SUCCESS)
        }
        Err(err) => {
            eprintln!("Error: {}", err.message);
            Ok(output::exit_code::INVALID_ARGS)
        }
    }
}

pub async fn list(json: bool) -> anyhow::Result<i32> {
    let client = daemon_conn::connect_or_autostart().await?;
    let resp = client
        .call(
            IPC_PROTOCOL_VERSION,
            methods::GET_CONFIG,
            serde_json::json!({}),
        )
        .await?;
    match unwrap_response(resp) {
        Ok(value) => {
            if json {
                output::print_json(&value);
                return Ok(output::exit_code::SUCCESS);
            }
            let mut pairs = Vec::new();
            flatten("", &value, &mut pairs);
            pairs.sort();
            for (key, value) in pairs {
                if key == "schema_version" {
                    continue;
                }
                println!("{key} = {value}");
            }
            Ok(output::exit_code::SUCCESS)
        }
        Err(err) => {
            eprintln!("Error: {}", err.message);
            Ok(output::exit_code::GENERAL_ERROR)
        }
    }
}

fn flatten(prefix: &str, value: &serde_json::Value, out: &mut Vec<(String, String)>) {
    match value {
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten(&key, v, out);
            }
        }
        serde_json::Value::Array(_) => {
            // Peer list has its own richer view via `lcp peers`.
        }
        other => out.push((prefix.to_string(), scalar_to_string(other))),
    }
}

fn scalar_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}
