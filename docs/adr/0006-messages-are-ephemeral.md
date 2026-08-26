# ADR-006: Messages Are Ephemeral

## Status

Accepted

## Context

Persisting message content to disk would require encryption-at-rest design, a storage format, migrations, and secure deletion — none of which the product needs, and all of which conflict with the "no database" and "local state" principles (spec §1.2).

## Decision

Conversation history, latest-incoming indexes, and the dedup cache live in RAM only (spec §6.2). Pairing/trust data is the only thing persisted across daemon restarts (spec §6.1). On restart, message history is empty by design, not a bug — `lcp copy`/`lcp pick` reflect that explicitly (spec §6.3).

## Consequences

No message database, no disk history, no data-at-rest exposure for message content, and no history to migrate across schema/version changes. The tradeoff is that a daemon restart loses in-flight conversation context; this is accepted product behavior (see [[0008-no-offline-queue]]) and must be covered by tests (spec §21 Scenario E) and documented for users, not "fixed" later without an ADR superseding this one.
