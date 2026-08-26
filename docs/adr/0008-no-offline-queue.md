# ADR-008: No Offline Queue

## Status

Accepted

## Context

Queuing outgoing messages for offline peers would require durable storage (conflicting with [[0006-messages-are-ephemeral]]), retry/expiry policy, and would create surprising "message shows up much later" behavior that the product explicitly wants to avoid (spec §2.2 non-goals).

## Decision

If a peer is unreachable or the ACK times out, `send` fails immediately and clearly. Retrying is a new explicit send attempt (spec §8.7), not a hidden background queue.

## Consequences

Send failures are immediate and legible: the user knows right away whether a message went through. There is no delivery guarantee across a peer's offline period, which must be documented for users (see `docs/troubleshooting.md`) so "friend didn't get my message" has an obvious, expected explanation.
