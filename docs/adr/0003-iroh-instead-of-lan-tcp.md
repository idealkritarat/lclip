# ADR-003: Iroh Instead of LAN TCP

## Status

Accepted

## Context

Users are commonly on different networks (different Wi-Fi, different countries), not a shared LAN. A raw TCP/mDNS design would only work for same-network peers and would require the project to build NAT traversal, hole punching, relay fallback, and transport encryption itself — all explicitly out of scope (spec §0.6).

## Decision

Use Iroh as the sole cross-network transport. Iroh provides endpoint identity, authenticated encrypted QUIC, NAT traversal/hole punching, direct-path selection, relay fallback, and address lookup. The application never implements these itself.

## Consequences

Device identity becomes an Iroh `EndpointId` (Ed25519 public key) rather than an IP address or LAN-discoverable name. All connection establishment, direct-vs-relay selection, and transport encryption are delegated to Iroh; the application only owns pairing, trust, message framing, and CLI/UI (see [[0005-daemon-owns-network-state]]).
