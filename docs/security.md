# Security

## Boundary

Untrusted input sources: pairing tickets, remote endpoint connections, remote message frames, peer display names, local IPC frames, config file contents, CLI arguments/stdin. All of it is length-limited and validated before allocation or use.

## Cryptography

All transport encryption, key exchange, and NAT traversal come from Iroh's authenticated QUIC — nothing is layered on top, and no primitive is implemented in this codebase (spec §14.2, §0.6). Application-level crypto is limited to well-scoped, standard operations, each via a mature crate rather than a hand-rolled implementation:

| Operation | Crate | Notes |
|---|---|---|
| CSPRNG for invite secrets/nonces | `rand` (`OsRng`) | 32-byte invite secrets |
| Constant-time secret comparison | `subtle` | invite-secret verification |
| Verification-string derivation | `blake3` (keyed hash) | never used as an encryption key |
| Secret buffer zeroing | `zeroize` | identity secret bytes, invite secrets in memory |

## Authorization

- The Iroh `EndpointId` is the canonical authorization principal. Local alias and any device/display name are untrusted labels, never identity.
- Normal message connections are accepted only from `EndpointId`s already in the trusted-peer store; unknown endpoints get a generic unauthorized response and the connection is closed.
- The pairing protocol only accepts `PairRequest` frames while an invite is active for that inviter; trust commits only after both sides confirm (two-sided confirmation, spec §7.5/§7.7).
- `lcp unpair` revokes trust and closes the active connection immediately.

## Secret storage

The Iroh secret key is generated once and stored via the OS credential store (macOS Keychain, Windows Credential Manager) through the `keyring` crate — never written to the config file, never printed, never exposed over IPC. If the credential store is locked or unavailable, the daemon fails safely rather than falling back to a weaker storage path. If the secret is missing but trusted peers exist in config, the daemon reports that identity recovery/re-pairing is required rather than silently generating a new, unrelated identity.

## Local IPC security

The socket/pipe is restricted to the current user (filesystem mode `0600` on macOS; a Windows security descriptor scoped to the current user SID on Windows, built via `windows-sys`). Every frame's IPC version and length are validated; unknown methods are rejected explicitly rather than ignored. Shutdown/reset-style methods still require a normal current-user IPC connection — there is no separate elevated control channel.

## Logging and privacy

Allowed: event type, a truncated/non-secret `EndpointId` prefix, connection state, direct/relay path, byte counts, error categories, timings.

Never logged: message text, clipboard text, the full pairing ticket, the invite secret, the private key, or a full config dump. This is enforced by convention and reviewed explicitly before each release (spec §14.8) — logging code should pass structured, pre-selected fields to `tracing`, never a whole payload struct via `Debug`/`Display` on types that may carry message content or secrets.

## Resource protection

Hard frame/message size limits (network and IPC), connection/stream timeouts, a cap on concurrent unauthenticated pairing attempts per invite, bounded history/dedup/queue sizes everywhere, and cancellation of per-connection tasks on disconnect — no unbounded task spawns driven by remote input.

## Dependencies handling secrets, networking, or IPC

| Crate | Role |
|---|---|
| `iroh` / `iroh-tickets` | Transport, encryption, NAT traversal, relay, ticket encoding of dialing info |
| `keyring` | OS credential store access for the identity secret key |
| `tokio` (`net::UnixListener`/`windows::named_pipe`) | Local IPC transport |
| `windows-sys` | Building a current-user-only security descriptor for the Windows named pipe |
| `rand`, `subtle`, `blake3`, `zeroize` | Invite-secret generation/comparison, verification-string derivation, secret zeroing |

## Pre-release review gate (spec §14.8)

Run `cargo audit`; review every `unsafe` block with a goal of zero application-authored `unsafe`; fuzz or targeted-test the ticket parser and frame decoder against malformed/oversized input; test an unauthorized `EndpointId` being rejected; test an expired and a reused invite both failing; grep logs/config for accidental secret or message-content leakage.
