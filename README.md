# LCP

LCP sends code and UTF-8 plain text between friends' machines, fast — across macOS and Windows, even on different networks. Pair once with a ticket, then:

```bash
lcp send First     # send your current clipboard to "First"
lcp copy First     # pull First's latest message into your clipboard
lcp fetch First     # print First's latest message to stdout, for piping
lcp pick First     # interactively choose an older message to copy
```

No account, no server, no message database. `lanclipd` runs in the background per user and keeps receiving even when no terminal is open; direct peer-to-peer when possible, encrypted relay fallback when not, via [Iroh](https://docs.iroh.computer/).

See [LCP-Agentic-Implementation-Spec.md](../LCP-Agentic-Implementation-Spec.md) for the full normative specification this implementation follows, and `docs/` for architecture, protocol, and security detail.

## Status

Under active implementation, following the spec's phase order (repository foundation → daemon/IPC → in-memory messaging → Iroh pairing → realtime transport → security hardening → macOS UI → release packaging). See `docs/adr/` for the architectural decisions already locked in.

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
