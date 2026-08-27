# ADR-007: Clipboard Access Stays in Clients

## Status

Accepted

## Context

Background/service processes on Windows may not share the interactive desktop's clipboard session, and clipboard access is inherently a foreground-user-session concern. Putting clipboard code in the daemon would also make the daemon harder to test headlessly.

## Decision

Clipboard reads/writes happen only in the foreground CLI process (`arboard`), never in `lanclipd`. The daemon only ever sees message text passed to it over IPC.

## Consequences

The daemon stays headless and unit-testable without a display/session. Future clients must implement their own clipboard integration against the same IPC contract; there is no shared clipboard code path through the daemon.
