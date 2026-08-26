//! On-disk config: schema, atomic I/O, and the dotted-key get/set backing `lcp config` (spec §6, §11.3).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use lcp_protocol::CONFIG_SCHEMA_VERSION;

use crate::peers::TrustedPeer;

pub const MIN_HISTORY_LIMIT: u32 = 20;
pub const MAX_HISTORY_LIMIT: u32 = 500;
pub const DEFAULT_HISTORY_LIMIT: u32 = 100;
pub const HARD_MAX_MESSAGE_BYTES: u64 = 5 * 1024 * 1024;

/// Keys writable via `lcp config set` (spec §11.3). No other dotted path may be set.
pub const WRITABLE_KEYS: &[&str] = &[
    "user.name",
    "user.device_name",
    "history.limit_per_peer",
    "daemon.autostart",
    "network.relay_mode",
];

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse config: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("unsupported config schema version {found} (expected {expected})")]
    UnsupportedSchemaVersion { found: u32, expected: u32 },
    #[error("could not determine platform config directory")]
    NoConfigDir,
    #[error("unknown or read-only config key {0:?}")]
    UnknownKey(String),
    #[error("invalid value for {key:?}: {reason}")]
    InvalidValue { key: String, reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub schema_version: u32,
    pub user: UserConfig,
    pub history: HistoryConfig,
    pub message: MessageConfig,
    pub daemon: DaemonConfig,
    pub network: NetworkConfig,
    #[serde(default)]
    pub trusted_peers: Vec<TrustedPeer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserConfig {
    pub name: String,
    pub device_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryConfig {
    pub limit_per_peer: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageConfig {
    pub max_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    pub autostart: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub relay_mode: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            user: UserConfig {
                name: default_user_name(),
                device_name: default_device_name(),
            },
            history: HistoryConfig {
                limit_per_peer: DEFAULT_HISTORY_LIMIT,
            },
            message: MessageConfig {
                max_bytes: HARD_MAX_MESSAGE_BYTES,
            },
            daemon: DaemonConfig { autostart: true },
            network: NetworkConfig {
                relay_mode: "default".to_string(),
            },
            trusted_peers: Vec::new(),
        }
    }
}

fn default_user_name() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "Me".to_string())
}

fn default_device_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "My Device".to_string())
}

/// `~/Library/Application Support/lcp` on macOS, `%APPDATA%\lcp` on Windows. Overridable via
/// `LCP_PROFILE_DIR` -- a dev/test-only escape hatch (not a documented user-facing setting) so
/// multiple `lanclipd` "machines" can run under distinct identities on one dev box.
pub fn app_dir() -> Result<PathBuf, ConfigError> {
    if let Ok(dir) = std::env::var("LCP_PROFILE_DIR") {
        return Ok(PathBuf::from(dir));
    }
    let base = directories::BaseDirs::new().ok_or(ConfigError::NoConfigDir)?;
    Ok(base.config_dir().join("lcp"))
}

pub fn config_file_path() -> Result<PathBuf, ConfigError> {
    Ok(app_dir()?.join("config.json"))
}

/// `~/Library/Logs/lcp` on macOS, `%LOCALAPPDATA%\lcp\logs` on Windows (or `LCP_PROFILE_DIR`,
/// see [`app_dir`]).
pub fn logs_dir() -> Result<PathBuf, ConfigError> {
    if let Ok(dir) = std::env::var("LCP_PROFILE_DIR") {
        return Ok(PathBuf::from(dir).join("logs"));
    }
    let base = directories::BaseDirs::new().ok_or(ConfigError::NoConfigDir)?;
    if cfg!(target_os = "macos") {
        Ok(base.home_dir().join("Library").join("Logs").join("lcp"))
    } else {
        Ok(base.data_local_dir().join("lcp").join("logs"))
    }
}

impl Config {
    /// Loads config from disk, creating and persisting a default config if none exists yet.
    pub fn load_or_create() -> Result<Self, ConfigError> {
        let path = config_file_path()?;
        if !path.exists() {
            let config = Config::default();
            config.save()?;
            return Ok(config);
        }
        let bytes = std::fs::read(&path)?;
        let config: Config = serde_json::from_slice(&bytes)?;
        if config.schema_version != CONFIG_SCHEMA_VERSION {
            return Err(ConfigError::UnsupportedSchemaVersion {
                found: config.schema_version,
                expected: CONFIG_SCHEMA_VERSION,
            });
        }
        Ok(config)
    }

    /// Writes config atomically: write to a temp file in the same directory, flush, then
    /// rename over the real path, so a crash mid-write never leaves a truncated config.
    pub fn save(&self) -> Result<(), ConfigError> {
        let path = config_file_path()?;
        let dir = path.parent().ok_or(ConfigError::NoConfigDir)?;
        std::fs::create_dir_all(dir)?;
        let tmp_path = dir.join(format!(".config.json.tmp-{}", std::process::id()));
        let json = serde_json::to_vec_pretty(self)?;
        {
            use std::io::Write;
            let mut file = std::fs::File::create(&tmp_path)?;
            file.write_all(&json)?;
            file.sync_all()?;
        }
        std::fs::rename(&tmp_path, &path)?;
        Ok(())
    }

    pub fn get_by_key(&self, key: &str) -> Option<serde_json::Value> {
        let value = serde_json::to_value(self).ok()?;
        key.split('.').try_fold(value, |acc, part| match acc {
            serde_json::Value::Object(mut map) => map.remove(part),
            _ => None,
        })
    }

    /// Sets one of [`WRITABLE_KEYS`] from a raw string (as received from the CLI). Rejects any
    /// other key, and rejects values that don't parse into that key's type.
    pub fn set_by_key(&mut self, key: &str, raw_value: &str) -> Result<(), ConfigError> {
        if !WRITABLE_KEYS.contains(&key) {
            return Err(ConfigError::UnknownKey(key.to_string()));
        }
        match key {
            "user.name" => self.user.name = raw_value.to_string(),
            "user.device_name" => self.user.device_name = raw_value.to_string(),
            "history.limit_per_peer" => {
                let n: u32 = raw_value.parse().map_err(|_| ConfigError::InvalidValue {
                    key: key.to_string(),
                    reason: "must be an integer".to_string(),
                })?;
                if !(MIN_HISTORY_LIMIT..=MAX_HISTORY_LIMIT).contains(&n) {
                    return Err(ConfigError::InvalidValue {
                        key: key.to_string(),
                        reason: format!(
                            "must be between {MIN_HISTORY_LIMIT} and {MAX_HISTORY_LIMIT}"
                        ),
                    });
                }
                self.history.limit_per_peer = n;
            }
            "daemon.autostart" => {
                let b: bool = raw_value.parse().map_err(|_| ConfigError::InvalidValue {
                    key: key.to_string(),
                    reason: "must be true or false".to_string(),
                })?;
                self.daemon.autostart = b;
            }
            "network.relay_mode" => self.network.relay_mode = raw_value.to_string(),
            _ => unreachable!("checked against WRITABLE_KEYS above"),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_current_schema_version() {
        assert_eq!(Config::default().schema_version, CONFIG_SCHEMA_VERSION);
    }

    #[test]
    fn get_by_key_reads_nested_field() {
        let config = Config::default();
        assert_eq!(
            config.get_by_key("user.name").unwrap(),
            serde_json::Value::String(config.user.name.clone())
        );
    }

    #[test]
    fn get_by_key_returns_none_for_unknown_path() {
        let config = Config::default();
        assert!(config.get_by_key("does.not.exist").is_none());
    }

    #[test]
    fn set_by_key_updates_writable_fields() {
        let mut config = Config::default();
        config.set_by_key("user.name", "Ideal").unwrap();
        assert_eq!(config.user.name, "Ideal");

        config.set_by_key("daemon.autostart", "false").unwrap();
        assert!(!config.daemon.autostart);

        config.set_by_key("history.limit_per_peer", "200").unwrap();
        assert_eq!(config.history.limit_per_peer, 200);
    }

    #[test]
    fn set_by_key_rejects_unknown_key() {
        let mut config = Config::default();
        assert!(matches!(
            config.set_by_key("message.max_bytes", "1"),
            Err(ConfigError::UnknownKey(_))
        ));
    }

    #[test]
    fn set_by_key_rejects_out_of_range_history_limit() {
        let mut config = Config::default();
        assert!(config.set_by_key("history.limit_per_peer", "1").is_err());
        assert!(config
            .set_by_key("history.limit_per_peer", "10000")
            .is_err());
    }

    #[test]
    fn set_by_key_rejects_non_bool_autostart() {
        let mut config = Config::default();
        assert!(config.set_by_key("daemon.autostart", "yes").is_err());
    }

    #[test]
    fn round_trips_through_json() {
        let config = Config::default();
        let json = serde_json::to_string(&config).unwrap();
        let decoded: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.schema_version, config.schema_version);
        assert_eq!(decoded.user.name, config.user.name);
    }
}
