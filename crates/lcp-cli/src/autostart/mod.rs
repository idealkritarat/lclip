//! Per-user autostart registration (spec §16): a Windows `Run` key, or a macOS LaunchAgent.
//! No administrator privileges required by either path.

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{install, uninstall};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{install, uninstall};
