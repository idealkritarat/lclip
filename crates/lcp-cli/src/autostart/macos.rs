//! Per-user autostart via a LaunchAgent plist in `~/Library/LaunchAgents` (spec §16.2).
//!
//! Not compiled or tested on this Windows development machine -- verify on real macOS/CI
//! before relying on it.

const LABEL: &str = "com.lcp.lanclipd";

fn home_dir() -> anyhow::Result<std::path::PathBuf> {
    std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .map_err(|_| anyhow::anyhow!("HOME is not set"))
}

fn plist_path() -> anyhow::Result<std::path::PathBuf> {
    Ok(home_dir()?
        .join("Library/LaunchAgents")
        .join(format!("{LABEL}.plist")))
}

fn daemon_path() -> anyhow::Result<std::path::PathBuf> {
    let exe = std::env::current_exe()?;
    Ok(exe.with_file_name("lanclipd"))
}

pub fn install() -> anyhow::Result<()> {
    let logs_dir = home_dir()?.join("Library/Logs/lcp");
    std::fs::create_dir_all(&logs_dir)?;
    let plist_path = plist_path()?;
    std::fs::create_dir_all(plist_path.parent().unwrap())?;

    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{daemon}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{logs}/lanclipd.stdout.log</string>
    <key>StandardErrorPath</key>
    <string>{logs}/lanclipd.stderr.log</string>
</dict>
</plist>
"#,
        daemon = daemon_path()?.display(),
        logs = logs_dir.display(),
    );
    std::fs::write(&plist_path, plist)?;

    let status = std::process::Command::new("launchctl")
        .args(["load", "-w"])
        .arg(&plist_path)
        .status()?;
    anyhow::ensure!(status.success(), "`launchctl load` failed with {status}");
    Ok(())
}

pub fn uninstall() -> anyhow::Result<()> {
    let plist_path = plist_path()?;
    if plist_path.exists() {
        let _ = std::process::Command::new("launchctl")
            .args(["unload", "-w"])
            .arg(&plist_path)
            .status();
        std::fs::remove_file(&plist_path)?;
    }
    Ok(())
}
