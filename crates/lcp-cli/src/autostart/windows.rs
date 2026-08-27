//! Per-user autostart via `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`. Chosen over a
//! Scheduled Task for simplicity (spec §0's tie-break rule) -- no elevation, no task-scheduler
//! concepts, just one registry value naming the daemon binary.

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "LCP";

fn daemon_path() -> anyhow::Result<std::path::PathBuf> {
    let exe = std::env::current_exe()?;
    Ok(exe.with_file_name("lanclipd.exe"))
}

pub fn install() -> anyhow::Result<()> {
    let daemon_path = daemon_path()?;
    let daemon_command = format!("\"{}\"", daemon_path.display());
    let status = std::process::Command::new("reg")
        .args([
            "add",
            RUN_KEY,
            "/v",
            VALUE_NAME,
            "/t",
            "REG_SZ",
            "/d",
            &daemon_command,
            "/f",
        ])
        .status()?;
    anyhow::ensure!(status.success(), "`reg add` failed with {status}");
    Ok(())
}

pub fn uninstall() -> anyhow::Result<()> {
    let status = std::process::Command::new("reg")
        .args(["delete", RUN_KEY, "/v", VALUE_NAME, "/f"])
        .status()?;
    // Not-found is fine here (nothing to uninstall); only a hard launch failure is an error.
    if !status.success() {
        anyhow::ensure!(
            status.code() == Some(1),
            "`reg delete` failed with {status}"
        );
    }
    Ok(())
}
