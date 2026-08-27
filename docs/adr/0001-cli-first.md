# ADR-001: CLI First

## Status

Accepted

## Context

The project needs a daemon/CLI core that can be tested independently on macOS and Windows. Keeping the CLI path first prevents release-critical behavior (pairing, messaging, reconnect) from depending on any graphical client.

## Decision

Implement and release `lanclipd` and `lcp` as the primary product surface. Any graphical client is separate scope and must build on the same daemon/IPC contract.

## Consequences

Core behavior is independently testable on both platforms via CLI and automated tests. Future clients can be added without changing the daemon's network ownership model.
