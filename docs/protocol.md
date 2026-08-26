# Protocol

Two independent, versioned protocols exist: the **network protocol** (between `lanclipd` instances, over Iroh) and the **local IPC protocol** (between a client and its own `lanclipd`, over a Unix socket or Windows named pipe). They are versioned separately and never conflated.

## Network protocol

- ALPN: `lcp/1`. A protocol version mismatch fails clearly; it never silently downgrades.
- One Iroh `Endpoint` per daemon. Identity is the Iroh `EndpointId` (Ed25519 public key).
- At most one preferred authenticated connection per trusted peer. If both sides dial simultaneously, the connection is resolved deterministically by sorted `EndpointId`: the lower id owns the outbound preferred connection, the other is closed after authentication.
- One bidirectional QUIC stream per request/message operation — no custom multiplexing layer over a single long-lived stream. For a text message: open stream → write one framed `NetworkEnvelope` → finish send side → receiver reads with a strict byte limit → validate/store → write `AckPayload` on the same stream → sender marks sent.

### Wire envelope

```rust
struct NetworkEnvelope {
    protocol_version: u16,
    request_id: Uuid,
    body: NetworkBody,
}

enum NetworkBody {
    Text(TextPayload),
    Ack(AckPayload),
    Ping,
    Pong,
    PairRequest(PairRequest),
    PairDecision(PairDecision),
    Error(NetworkErrorPayload),
}
```

Pairing and messaging share the same ALPN and the same envelope type — there is no separate pairing protocol/port; a daemon dispatches on the `NetworkBody` variant. Pairing-variant frames are only accepted while an invite is active (see Security §Authorization).

### Framing and limits

- Postcard-serialize the envelope, prefix with a 4-byte big-endian length.
- Hard cap: 6 MiB per frame; max UTF-8 text payload: 5 MiB (5,242,880 bytes).
- Zero-length and oversized frames are rejected before allocation. Reads are bounded and time out.
- Connect attempt timeout 8s; send timeout 5s once connected; ACK timeout 5s.

### Delivery and ordering

- No offline queue. Sender assigns a UUID v4 message id; retries reuse it.
- Receiver keeps a bounded dedup set; a duplicate id from the same `EndpointId` returns the original ACK without re-adding history.
- Receiver assigns a monotonically increasing `receive_sequence` on acceptance — this, not sender-supplied timestamps (clocks may differ), determines "latest" for `copy`/`fetch` without a peer argument.

### Reconnect

Backoff schedule: 250ms, 500ms, 1s, 2s, 5s, then capped at 10s, plus 10–25% jitter. Reset after a stable authenticated connection. An explicit `send` always triggers an immediate retry attempt regardless of backoff state.

## Pairing ticket (`PairingTicketV1`)

```rust
struct PairingTicketV1 {
    version: u8,
    endpoint_ticket: String,       // encoded Iroh EndpointTicket (dialing info)
    invite_secret: [u8; 32],
    expires_at_unix_ms: i64,
    inviter_display_name: String,
    inviter_device_name: String,
}
```

Encoding: Postcard, then URL-safe Base64 without padding, prefixed with `lcp1_`. TTL is bounded 60–900s (default 300s). The parser rejects unknown versions, malformed input, and oversized tickets before doing anything else with them. A ticket is treated as sensitive connection metadata (see `docs/security.md`) — never logged, never in crash reports.

Verification string (spec §7.6): derived via a keyed BLAKE3 hash over both `EndpointId`s (sorted), the invite secret, both handshake nonces, and the context string `lcp-pairing-v1`; rendered as three words plus four digits (e.g. `mango-river-pencil-4821`). It is a human confirmation aid only — never used as a key.

## Local IPC protocol

- Transport: Unix domain socket on macOS (`0600`, current-user only), named pipe on Windows (ACL restricted to the current user's SID). No loopback TCP fallback.
- Framing: 4-byte big-endian length prefix + JSON UTF-8 body, hard-capped at 6 MiB.
- Three frame kinds: `IpcRequest` (client→daemon, correlated by `id`), `IpcResponse` (daemon→client, matches a request `id`), `IpcEvent` (daemon→client, unsolicited, best-effort).
- Clients subscribing to events (`subscribe`) must still re-fetch snapshot state after reconnecting to the daemon — events are never the sole source of truth (spec §10.6).

See `crates/lcp-protocol/src/ipc.rs` for the exact request/response/event Rust types and `docs/architecture.md` for which crate owns transport vs. types.
