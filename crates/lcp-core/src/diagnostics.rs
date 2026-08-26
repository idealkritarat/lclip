//! `lcp doctor` checks (spec §17.2, §11.11). Client-only checks (daemon running, IPC
//! reachability) live in `lcp-cli` since they apply precisely when there's no daemon to ask;
//! everything the daemon itself can inspect lives here.

use serde::Serialize;

use crate::state::SharedState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Ok,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticResult {
    pub id: String,
    pub severity: Severity,
    pub summary: String,
    pub detail: String,
    pub suggested_action: Option<String>,
}

impl DiagnosticResult {
    fn ok(id: &str, summary: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            id: id.to_string(),
            severity: Severity::Ok,
            summary: summary.into(),
            detail: detail.into(),
            suggested_action: None,
        }
    }

    fn warning(
        id: &str,
        summary: impl Into<String>,
        detail: impl Into<String>,
        suggested_action: impl Into<String>,
    ) -> Self {
        Self {
            id: id.to_string(),
            severity: Severity::Warning,
            summary: summary.into(),
            detail: detail.into(),
            suggested_action: Some(suggested_action.into()),
        }
    }

    fn error(
        id: &str,
        summary: impl Into<String>,
        detail: impl Into<String>,
        suggested_action: impl Into<String>,
    ) -> Self {
        Self {
            id: id.to_string(),
            severity: Severity::Error,
            summary: summary.into(),
            detail: detail.into(),
            suggested_action: Some(suggested_action.into()),
        }
    }
}

/// Runs every daemon-side check. Never sends anything to a peer (spec §17.2: "Doctor must not
/// send test message content to peers").
pub async fn run_checks(state: &SharedState, endpoint: &iroh::Endpoint) -> Vec<DiagnosticResult> {
    let mut results = Vec::new();

    {
        let state = state.read().await;
        let endpoint_id = state.identity.endpoint_id().to_string();
        results.push(DiagnosticResult::ok(
            "identity",
            "Identity secret loaded",
            format!(
                "Endpoint ID prefix: {}",
                &endpoint_id[..12.min(endpoint_id.len())]
            ),
        ));
    }

    let online = tokio::time::timeout(std::time::Duration::from_secs(5), endpoint.online())
        .await
        .is_ok();
    results.push(if online {
        DiagnosticResult::ok(
            "endpoint_online",
            "Iroh endpoint is online",
            "A relay handshake has completed.",
        )
    } else {
        DiagnosticResult::warning(
            "endpoint_online",
            "Iroh endpoint is not online yet",
            "No relay handshake completed within 5 seconds.",
            "Check internet connectivity and any firewall blocking outbound UDP/QUIC.",
        )
    });

    {
        let state = state.read().await;
        let mode = state.config.network.relay_mode.clone();
        results.push(DiagnosticResult::ok(
            "relay_mode",
            format!("Using {mode} relay configuration"),
            "Public Iroh relays have no production SLA (spec §3.5).",
        ));
    }

    {
        let state = state.read().await;
        if state.config.schema_version == lcp_protocol::CONFIG_SCHEMA_VERSION {
            results.push(DiagnosticResult::ok(
                "config_schema",
                "Config schema is current",
                format!("schema_version = {}", state.config.schema_version),
            ));
        } else {
            results.push(DiagnosticResult::error(
                "config_schema",
                "Config schema version mismatch",
                format!(
                    "found {}, expected {}",
                    state.config.schema_version,
                    lcp_protocol::CONFIG_SCHEMA_VERSION
                ),
                "Back up ~/.../lcp/config.json, then remove or migrate it.",
            ));
        }
    }

    {
        let state = state.read().await;
        let mut seen = std::collections::HashSet::new();
        let mut duplicates = Vec::new();
        for peer in &state.config.trusted_peers {
            if !seen.insert(peer.alias.to_lowercase()) {
                duplicates.push(peer.alias.clone());
            }
        }
        results.push(if duplicates.is_empty() {
            DiagnosticResult::ok(
                "peer_aliases",
                "Trusted peer aliases are unique",
                format!("{} paired peer(s)", state.config.trusted_peers.len()),
            )
        } else {
            DiagnosticResult::error(
                "peer_aliases",
                "Duplicate trusted peer aliases found",
                format!("Duplicated: {}", duplicates.join(", ")),
                "Edit the config file directly to rename one of the duplicates.",
            )
        });
    }

    results.push(autostart_check());

    results
}

#[cfg(windows)]
fn autostart_check() -> DiagnosticResult {
    let installed = std::process::Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
            "/v",
            "LCP",
        ])
        .output()
        .is_ok_and(|o| o.status.success());
    if installed {
        DiagnosticResult::ok(
            "autostart",
            "Autostart is installed",
            "A per-user Run registry entry points at lanclipd.exe.",
        )
    } else {
        DiagnosticResult {
            id: "autostart".to_string(),
            severity: Severity::Warning,
            summary: "Autostart is not installed".to_string(),
            detail: "lanclipd will not start automatically at login.".to_string(),
            suggested_action: Some("Run `lcp daemon install` if you want it to.".to_string()),
        }
    }
}

#[cfg(target_os = "macos")]
fn autostart_check() -> DiagnosticResult {
    let installed = std::env::var("HOME")
        .map(|home| {
            std::path::Path::new(&home)
                .join("Library/LaunchAgents/com.lcp.lanclipd.plist")
                .exists()
        })
        .unwrap_or(false);
    if installed {
        DiagnosticResult::ok(
            "autostart",
            "Autostart is installed",
            "A LaunchAgent plist is present in ~/Library/LaunchAgents.",
        )
    } else {
        DiagnosticResult {
            id: "autostart".to_string(),
            severity: Severity::Warning,
            summary: "Autostart is not installed".to_string(),
            detail: "lanclipd will not start automatically at login.".to_string(),
            suggested_action: Some("Run `lcp daemon install` if you want it to.".to_string()),
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn autostart_check() -> DiagnosticResult {
    DiagnosticResult {
        id: "autostart".to_string(),
        severity: Severity::Warning,
        summary: "Autostart check is not implemented on this platform".to_string(),
        detail: String::new(),
        suggested_action: None,
    }
}
