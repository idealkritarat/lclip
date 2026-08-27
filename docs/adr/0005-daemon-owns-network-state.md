# ADR-005: Daemon Owns Network State

## Status

Accepted

## Context

A short-lived CLI process cannot receive messages in real time or maintain a stable Iroh endpoint identity between invocations. Realtime receive (spec §1.2) requires something that keeps running.

## Decision

`lanclipd` is a long-running per-user daemon and the single owner of all network/session state (Iroh endpoint, trusted peers, active invitations, connections, in-memory conversation history, dedup cache, reconnect tasks). `lcp` is a stateless client that talks to it over local IPC and never opens its own Iroh endpoint.

## Consequences

Exactly one process per user holds the Iroh identity and can receive while no terminal is open. The CLI must tolerate the daemon restarting and must never duplicate network state locally.
