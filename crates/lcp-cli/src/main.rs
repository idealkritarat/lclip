mod autostart;
mod commands;
mod daemon_conn;
mod output;

use clap::{Parser, Subcommand};
use lcp_protocol::ipc::IpcResponse;

#[derive(Parser)]
#[command(
    name = "lcp",
    version,
    about = "Send code and text between paired machines, fast."
)]
struct Cli {
    /// Structured output for commands that support it. Never wraps raw `fetch` content.
    #[arg(long, global = true)]
    json: bool,
    #[arg(long, global = true)]
    quiet: bool,
    #[arg(long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show daemon, identity, relay, and peer status.
    Status,
    /// List trusted peers and their connection status.
    Peers,
    /// Read or write local config.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Manage the lanclipd background daemon and its autostart registration.
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    Get { key: String },
    Set { key: String, value: String },
    List,
}

#[derive(Subcommand)]
enum DaemonAction {
    Status,
    Start,
    Stop,
    Restart,
    Install,
    Uninstall,
}

/// Unwraps an `IpcResponse` into its result value, or a plain error message string.
pub fn unwrap_response(response: IpcResponse) -> Result<serde_json::Value, String> {
    if response.ok {
        Ok(response.result.unwrap_or(serde_json::Value::Null))
    } else {
        let message = response
            .error
            .map(|e| format!("[{}] {}", e.code, e.message))
            .unwrap_or_else(|| "unknown error".to_string());
        Err(message)
    }
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let code = match cli.command {
        Command::Status => commands::status::run(cli.json).await,
        Command::Peers => commands::peers::run(cli.json).await,
        Command::Config { action } => match action {
            ConfigAction::Get { key } => commands::config::get(&key, cli.json).await,
            ConfigAction::Set { key, value } => commands::config::set(&key, &value).await,
            ConfigAction::List => commands::config::list(cli.json).await,
        },
        Command::Daemon { action } => match action {
            DaemonAction::Status => commands::daemon::status().await,
            DaemonAction::Start => commands::daemon::start().await,
            DaemonAction::Stop => commands::daemon::stop().await,
            DaemonAction::Restart => commands::daemon::restart().await,
            DaemonAction::Install => commands::daemon::install().await,
            DaemonAction::Uninstall => commands::daemon::uninstall().await,
        },
    };

    match code {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(output::exit_code::GENERAL_ERROR);
        }
    }
}
