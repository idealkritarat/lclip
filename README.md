# LCP

LCP sends code and UTF-8 plain text between friends' machines, fast — across macOS and Windows, even on different networks. Pair once with a ticket, then:

```bash
lcp invite               # generate a pairing ticket and wait for a peer
lcp pair <ticket>         # pair using a ticket a friend sent you
lcp send First            # send your current clipboard to "First"
lcp copy First            # pull First's latest message into your clipboard
lcp fetch First           # print First's latest message to stdout, for piping
lcp pick First            # interactively choose an older message to copy
lcp peers                 # list paired peers and their live status
lcp doctor                # check daemon/identity/connectivity health
```

No account, no server, no message database. `lanclipd` runs in the background per user and keeps receiving even when no terminal is open; direct peer-to-peer when possible, encrypted relay fallback when not, via [Iroh](https://docs.iroh.computer/).

See [LCP-Agentic-Implementation-Spec.md](LCP-Agentic-Implementation-Spec.md) for the full normative specification this implementation follows, and `docs/` for architecture, protocol, and security detail.

## Status

The cross-platform Rust core (Phases 0-5 of the spec: workspace, daemon/IPC, in-memory messaging, real Iroh pairing and messaging, connection reuse and status, security/reliability hardening) is implemented and covered by 57 automated tests. It has been verified end-to-end with two live local daemons actually pairing and exchanging messages over a real Iroh connection.

Phase 6 now has a native macOS menu bar app source tree in `macos/LCPMenuBar/`. Phase 7 has per-user install/uninstall scripts, packaging scripts, checksums, and CI configuration. macOS-specific pieces were authored from a Windows environment and still need real macOS/Xcode verification before release.

## Building from source

Requires a stable Rust toolchain (see `rust-toolchain.toml`).

```bash
cargo build --workspace
cargo test --workspace
```

On Windows, building `x86_64-pc-windows-msvc` (the default) requires the MSVC C++ build tools (Visual Studio Build Tools with the "Desktop development with C++" workload). See `docs/troubleshooting.md` if you hit a linker error.

Binaries: `lanclipd` (background daemon), `lcp` (CLI). The macOS menu bar app lives in `macos/LCPMenuBar/` and is built separately with Xcode.

Build the macOS menu bar app on macOS:

```bash
xcodebuild \
  -project macos/LCPMenuBar/LCPMenuBar.xcodeproj \
  -scheme LCPMenuBar \
  -configuration Release \
  CODE_SIGNING_ALLOWED=NO
```

## Install

Windows, per user:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\install-windows.ps1
```

macOS, per user:

```bash
./scripts/install-macos.sh
```

Both scripts build release binaries, copy `lcp` and `lanclipd` to a user-writable location, enable daemon autostart, and start the daemon. Uninstall removes autostart and installed binaries but leaves identity/config intact:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\uninstall-windows.ps1
```

```bash
./scripts/uninstall-macos.sh
```

## First Use

On machine A:

```bash
lcp invite
```

Send the printed ticket to machine B, then on machine B:

```bash
lcp pair <ticket>
```

After both sides confirm the same verification code:

```bash
lcp peers
lcp send <peer-alias>
lcp copy <peer-alias>
```

On Windows, allow the firewall prompt for `lanclipd` if it appears.

## Packaging

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\package-windows.ps1
```

```bash
./scripts/package-macos.sh
```

Artifacts are written to `dist/` with `.sha256` checksum files.

## Repository layout

```text
crates/lcp-protocol   pure wire/config/IPC types, no I/O
crates/lcp-core       Iroh endpoint, pairing, peer state, conversations
crates/lcp-ipc        cross-platform local IPC transport
crates/lanclipd       the daemon binary
crates/lcp-cli        the lcp CLI binary
macos/LCPMenuBar      native macOS menu bar UI (Swift/AppKit/SwiftUI)
docs/                 architecture, protocol, security, troubleshooting, ADRs
scripts/              install/uninstall helpers
```

## License

MIT — see [LICENSE](LICENSE).
