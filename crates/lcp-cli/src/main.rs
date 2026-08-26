mod autostart;
mod clipboard;
mod commands;
mod daemon_conn;
mod output;
mod picker;

use clap::{Parser, Subcommand};
use lcp_protocol::ipc::{IpcError, IpcResponse};

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
    /// Send the current clipboard (or --stdin/--text) to a paired peer.
    Send {
        peer: String,
        /// Read the message from stdin (EOF-terminated) instead of the clipboard.
        #[arg(long, conflicts_with = "text")]
        stdin: bool,
        /// Use this exact text instead of the clipboard.
        #[arg(long, conflicts_with = "stdin")]
        text: Option<String>,
    },
    /// Copy the latest incoming message (from `peer`, or from anyone) into the clipboard.
    Copy {
        peer: Option<String>,
        /// Open the interactive picker instead (alias for `lcp pick`).
        #[arg(short = 'l', long = "list")]
        list: bool,
        /// With --list, only show incoming messages.
        #[arg(long)]
        incoming: bool,
    },
    /// Print the latest incoming message (from `peer`, or from anyone) to stdout.
    Fetch { peer: Option<String> },
    /// Interactively choose a message to copy.
    Pick {
        peer: Option<String>,
        /// Only show incoming messages.
        #[arg(long)]
        incoming: bool,
    },
    /// Create a pairing invite and wait for someone to pair (spec §7.4).
    Invite {
        /// Invite lifetime in seconds (clamped to 60-900).
        #[arg(long, default_value_t = 300)]
        ttl: u64,
        /// Don't copy the ticket to the clipboard.
        #[arg(long)]
        no_copy: bool,
    },
    /// Pair with someone using a ticket they gave you (spec §7.5).
    Pair { ticket: String },
    /// Revoke trust in a paired peer (spec §7.8).
    Unpair {
        peer: String,
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
    },
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
    /// Check daemon health, identity, connectivity, and config.
    Doctor,
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

/// Unwraps an `IpcResponse` into its result value, or the structured `IpcError` so callers can
/// match on `.code` to pick the right exit code rather than parsing a flattened string.
pub fn unwrap_response(response: IpcResponse) -> Result<serde_json::Value, IpcError> {
    if response.ok {
        Ok(response.result.unwrap_or(serde_json::Value::Null))
    } else {
        Err(response.error.unwrap_or_else(|| IpcError {
            code: lcp_protocol::ipc::error_codes::INTERNAL.to_string(),
            message: "unknown error".to_string(),
        }))
    }
}

/// Maps an `IpcError.code` to its spec §11.13 exit code, falling back to a general error.
pub fn exit_code_for(error: &IpcError) -> i32 {
    use lcp_protocol::ipc::error_codes;
    match error.code.as_str() {
        c if c == error_codes::PEER_NOT_FOUND || c == error_codes::PEER_AMBIGUOUS => {
            output::exit_code::PEER_NOT_FOUND
        }
        c if c == error_codes::PEER_OFFLINE => output::exit_code::PEER_OFFLINE,
        c if c == error_codes::NO_MESSAGE => output::exit_code::NO_MESSAGE,
        c if c == error_codes::PAIRING_FAILED => output::exit_code::PAIRING_FAILURE,
        c if c == error_codes::LIMIT_EXCEEDED => output::exit_code::LIMIT_EXCEEDED,
        c if c == error_codes::VERSION_MISMATCH => output::exit_code::VERSION_MISMATCH,
        c if c == error_codes::CREDENTIAL_STORE_FAILURE => output::exit_code::PERMISSION_FAILURE,
        c if c == error_codes::INVALID_PARAMS => output::exit_code::INVALID_ARGS,
        _ => output::exit_code::GENERAL_ERROR,
    }
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let code = match cli.command {
        Command::Status => commands::status::run(cli.json).await,
        Command::Peers => commands::peers::run(cli.json).await,
        Command::Send { peer, stdin, text } => {
            commands::send::run(&peer, stdin, text.as_deref()).await
        }
        Command::Copy {
            peer,
            list,
            incoming,
        } => {
            if list {
                commands::pick::run(peer.as_deref(), incoming).await
            } else {
                commands::copy::run(peer.as_deref()).await
            }
        }
        Command::Fetch { peer } => commands::fetch::run(peer.as_deref(), cli.json).await,
        Command::Pick { peer, incoming } => commands::pick::run(peer.as_deref(), incoming).await,
        Command::Invite { ttl, no_copy } => commands::invite::run(ttl, no_copy).await,
        Command::Pair { ticket } => commands::pair::run(&ticket).await,
        Command::Unpair { peer, yes } => commands::unpair::run(&peer, yes).await,
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
        Command::Doctor => commands::doctor::run(cli.json).await,
    };

    match code {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(output::exit_code::GENERAL_ERROR);
        }
    }
}
