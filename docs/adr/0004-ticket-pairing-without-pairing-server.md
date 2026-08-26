# ADR-004: Ticket Pairing Without Pairing Server

## Status

Accepted

## Context

Cross-network pairing needs some way to exchange routable connection information. A short numeric code is not enough to identify a routable endpoint by itself unless resolved through a rendezvous/lookup server, which the project does not want to run or depend on (spec §3.5 — no application-owned pairing rendezvous server). A six-digit-only scheme would also weaken the identity-confirmation guarantee pairing depends on.

## Decision

Use a long, application-specific ticket (`PairingTicketV1`, see `docs/protocol.md`) carrying Iroh dialing information, the inviter's `EndpointId`, a random invite secret, an expiry, and non-sensitive display metadata. The ticket is copy-pasted once at pairing time; no rendezvous server is introduced.

## Consequences

Pairing has no server-side moving part to operate or trust, at the cost of a longer string to transfer than a 6-digit code. The one-time cost is acceptable because pairing happens once per relationship (spec §1.2 "Pair once"). Two-sided confirmation with a derived verification string (spec §7.6) is still required before trust is committed, so the ticket alone never grants trust.
