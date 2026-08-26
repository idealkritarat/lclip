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

The cross-platform Rust core (Phases 0-5 of the spec: workspace, daemon/IPC, in-memory messaging, real Iroh pairing and messaging, connection reuse and status, security/reliability hardening) is implemented and covered by 57 automated tests, and has been verified end-to-end with two live local daemons actually pairing and exchanging messages over a real Iroh connection (see `docs/adr/` for the architectural decisions behind it).

The native macOS menu bar UI (Phase 6) and release packaging (Phase 7) are in progress. Both are being developed from a Windows-only environment, so anything macOS-specific (the menu bar app itself, the LaunchAgent autostart path, `unix.rs`'s socket transport) is written to spec but has not been compiled or run on real macOS/Xcode -- it needs verification there before being relied on.

## Building from source

Requires a stable Rust toolchain (see `rust-toolchain.toml`).

```bash
cargo build --workspace
cargo test --workspace
```

On Windows, building `x86_64-pc-windows-msvc` (the default) requires the MSVC C++ build tools (Visual Studio Build Tools with the "Desktop development with C++" workload). See `docs/troubleshooting.md` if you hit a linker error.

Binaries: `lanclipd` (background daemon), `lcp` (CLI). The macOS menu bar app lives in `macos/LCPMenuBar/` and is built separately with Xcode.

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
