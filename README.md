# LCP

LCP is a small CLI tool for sending clipboard text or code between paired machines.

It is built around two binaries:

- `lanclipd`: a per-user background daemon that owns networking, pairing, peer state, and in-memory message history.
- `lcp`: a stateless CLI client that talks to the daemon over local IPC.

No account, no application server, no message database. Pair once with a ticket, then send text directly over Iroh P2P when possible, with encrypted relay fallback when needed.

## Install

### macOS

```bash
curl -fsSL https://raw.githubusercontent.com/idealkritarat/lclip/master/scripts/bootstrap-macos.sh | bash
```

### Windows

Run in PowerShell:

```powershell
irm -Uri https://raw.githubusercontent.com/idealkritarat/lclip/master/scripts/bootstrap-windows.ps1 -OutFile "$env:TEMP\lcp-install.ps1"; powershell -ExecutionPolicy Bypass -File "$env:TEMP\lcp-install.ps1"
```

The bootstrap installer first tries to install a prebuilt binary from the latest GitHub Release. If no matching release exists, it builds from source and installs a minimal Rust toolchain if needed.

Installed files:

- macOS: `~/.local/bin/lcp` and `~/.local/bin/lanclipd`
- Windows: `%LOCALAPPDATA%\Programs\LCP\lcp.exe` and `lanclipd.exe`

The installer also enables per-user daemon autostart and starts `lanclipd`.

## Update

Run the same install command again to update LCP. It replaces the installed binaries, keeps autostart enabled, and preserves your identity, config, and paired peers.

macOS:

```bash
lcp daemon stop
curl -fsSL https://raw.githubusercontent.com/idealkritarat/lclip/master/scripts/bootstrap-macos.sh | bash
```

Windows:

```powershell
lcp daemon stop
irm -Uri https://raw.githubusercontent.com/idealkritarat/lclip/master/scripts/bootstrap-windows.ps1 -OutFile "$env:TEMP\lcp-install.ps1"; powershell -ExecutionPolicy Bypass -File "$env:TEMP\lcp-install.ps1"
```

Stopping the daemon first avoids replacing a binary while it is still running, especially on Windows where running executables may be locked.

## Uninstall

Uninstall removes autostart and installed binaries. It does not delete your identity, config, or paired peers.

### macOS

```bash
curl -fsSL https://raw.githubusercontent.com/idealkritarat/lclip/master/scripts/uninstall-macos.sh | bash
```

### Windows

Run in PowerShell:

```powershell
irm -Uri https://raw.githubusercontent.com/idealkritarat/lclip/master/scripts/uninstall-windows.ps1 -OutFile "$env:TEMP\lcp-uninstall.ps1"; powershell -ExecutionPolicy Bypass -File "$env:TEMP\lcp-uninstall.ps1"
```

## Quick Start

Set readable names on each machine:

```bash
lcp config set user.name "Ideal"
lcp config set user.device_name "MacBook"
```

On the first machine:

```bash
lcp invite
```

Copy the printed ticket to the second machine:

```bash
lcp pair "<ticket>"
```

Both machines will show a verification string. If it matches, type `y` on both sides.

After pairing:

```bash
lcp peers
lcp send <peer-alias> --text "hello"
lcp fetch <peer-alias>
lcp copy <peer-alias>
```

## Common Commands

```bash
lcp status                 # daemon, identity, relay, and peer summary
lcp doctor                 # health checks and suggested fixes
lcp peers                  # paired peers and online/offline status
lcp invite                 # create a pairing ticket
lcp pair "<ticket>"         # join an invite
lcp send First             # send current clipboard to peer "First"
lcp send First --text "hi"  # send explicit text
lcp fetch First            # print latest incoming message
lcp copy First             # copy latest incoming message to clipboard
lcp pick First             # choose a message interactively
lcp unpair First           # remove a paired peer
```

Daemon controls:

```bash
lcp daemon status
lcp daemon start
lcp daemon stop
lcp daemon restart
lcp daemon install
lcp daemon uninstall
```

## Build From Source

Requires a stable Rust toolchain.

```bash
git clone https://github.com/idealkritarat/lclip.git
cd lclip
cargo build --workspace --release
cargo test --workspace
```

Install from a clone:

```bash
bash ./scripts/install-macos.sh
```

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\install-windows.ps1
```

On Windows, the default MSVC Rust toolchain requires Visual Studio Build Tools with the "Desktop development with C++" workload.

## Package

```bash
./scripts/package-macos.sh
```

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\package-windows.ps1
```

Artifacts are written to `dist/` with `.sha256` checksum files.

## Data Locations

- macOS config: `~/Library/Application Support/lcp/config.json`
- macOS logs: `~/Library/Logs/lcp/`
- macOS socket: `~/Library/Application Support/lcp/lanclipd.sock`
- Windows config: `%APPDATA%\lcp\config.json`
- Windows logs: `%LOCALAPPDATA%\lcp\logs\`

Message history is stored only in daemon memory and disappears when `lanclipd` exits.

## Repository Layout

```text
crates/lcp-protocol   wire, ticket, and IPC types
crates/lcp-core       Iroh endpoint, pairing, peers, state, conversations
crates/lcp-ipc        local IPC transport
crates/lanclipd       background daemon
crates/lcp-cli        CLI client
docs/                 protocol, architecture, security, troubleshooting
scripts/              install, uninstall, package, bootstrap
```

## License

MIT, see [LICENSE](LICENSE).
