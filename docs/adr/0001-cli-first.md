# ADR-001: CLI First

## Status

Accepted

## Context

The project needs both a daemon/CLI core and a native macOS UI. Building the UI first would make core behavior (pairing, messaging, reconnect) hard to test independently and would bias the protocol design toward one platform.

## Decision

Implement `lanclipd` and `lcp` before any GUI. The macOS menu bar app (Phase 6) is not started until the CLI can pair and exchange messages across all required macOS/Windows combinations.

## Consequences

Core behavior is independently testable on both platforms via CLI and automated tests before any UI code exists. The UI is guaranteed to be a thin client over an already-working IPC/network stack rather than co-evolving with it.
