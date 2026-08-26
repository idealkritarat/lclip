# Troubleshooting

## Daemon and CLI

**`lcp` says the daemon is unavailable.**
Run `lcp daemon status`. If it's not running, `lcp daemon start` (most commands also auto-start it when safe). Check logs at `~/Library/Logs/lcp/` (macOS) or `%LOCALAPPDATA%\lcp\logs\` (Windows). `lcp doctor` runs a fuller set of checks (daemon reachability, identity, Iroh endpoint online state, relay mode, config schema, duplicate peer aliases, autostart) in one command.

**A peer always shows "offline" in `lcp peers` even though messages go through.**
Status is updated opportunistically -- when you send to them, or when they connect to you -- not by a continuously-running background reconnect loop (that's a documented, deliberate simplification for now; see `crates/lcp-core/src/connection.rs`). If you haven't talked to a peer recently, their last-known status may be stale rather than wrong.

**`lcp copy`/`lcp fetch` returns "no messages received since daemon start".**
This is expected behavior, not a bug — message history is RAM-only and does not survive a daemon restart (see ADR [[0006-messages-are-ephemeral]]). Pairing/trust is unaffected by restarts.

**A peer never receives what I sent.**
There is no offline queue (ADR [[0008-no-offline-queue]]): if the peer's daemon isn't reachable, `send` fails immediately with a nonzero exit code rather than queuing silently. Check `lcp peers` for that peer's status before assuming delivery.

## Pairing

**`lcp pair <ticket>` fails immediately.**
The ticket may be expired (default TTL 5 minutes) or already used once (invites are single-use at the application layer). Ask the inviter to run `lcp invite` again.

**The verification strings don't match.**
Do not confirm. This means the connection was not established with the endpoint you think it was (or one side has a stale/incorrect ticket). Re-run `lcp invite`/`lcp pair` from scratch.

## Connectivity

**Peers stay "offline" even though both machines are online.**
Run `lcp doctor` — it checks daemon health, Iroh endpoint state, and relay/address-lookup reachability, and reports whether you're on the public Iroh relay or a custom one. Public relays have no production SLA (spec §3.5); occasional relay-side issues are possible and are outside this project's control.

**Windows Firewall prompts when the daemon first runs.**
Allow it on private networks. `lanclipd` needs outbound (and for direct/hole-punched paths, inbound) UDP/QUIC connectivity; blocking it forces relay-only operation, which still works but adds latency.

## Clipboard

**`lcp send` says clipboard is empty/non-text.**
Only plain UTF-8 text clipboard content is supported; image/rich-text clipboard contents are rejected rather than silently converted.

## Windows build environment (developer-machine note)

If `cargo build` fails with `error: linker `link.exe` not found`, the machine has no MSVC Build Tools installed (Visual Studio's C++ workload). Two options:

1. **Recommended for most contributors:** install "Build Tools for Visual Studio" (C++ build tools workload) and use the default `stable-x86_64-pc-windows-msvc` toolchain — this is what CI uses.
2. **No-admin-rights fallback:** if you have a working MinGW-w64 `gcc` on `PATH` but cannot install Visual Studio, install the GNU-host toolchain (`rustup toolchain install stable-x86_64-pc-windows-gnu`) and scope it to this repo only with `rustup override set stable-x86_64-pc-windows-gnu` run from the repo root — do not change the committed `rust-toolchain.toml`, since that file must stay platform-neutral for macOS/CI.

   With the GNU host, `iroh` and `iroh-relay` (both intended for wasm32 too) unconditionally declare `crate-type = ["lib", "cdylib"]`. GNU `ld`/`lld` auto-export every symbol in a dylib with no explicit `dllexport` markers, and this dependency graph has 65,000+ candidate symbols — over the PE format's hard 65,535-symbol export-table ceiling (`error: export ordinal too large`, or with `-fuse-ld=lld`, `error: too many exported symbols`). MSVC's linker never auto-exports, so this never happens there; it is purely a GNU-toolchain-on-Windows artifact. The fix is a machine-local `[patch.crates-io]` (in `%USERPROFILE%\.cargo\config.toml`, never in the repo) pointing each affected crate at a local copy with `cdylib` dropped from `crate-type` — nothing else changed:

   ```toml
   [patch.crates-io]
   iroh = { path = "C:/Users/<you>/.cargo/local-patches/iroh-<version>" }
   iroh-relay = { path = "C:/Users/<you>/.cargo/local-patches/iroh-relay-<version>" }
   ```

   Get each local copy via `Invoke-WebRequest https://crates.io/api/v1/crates/<name>/<version>/download -OutFile <name>-<version>.crate` then `tar -xzf`, and remove `"cdylib"` from the extracted `Cargo.toml`'s `[lib]` section. If a future `iroh`/`iroh-relay` version bump surfaces the same error again, or another n0-computer crate does, repeat the same patch for that crate.
