# ADR-002: Native macOS UI, No Windows GUI

## Status

Accepted

## Context

Building and maintaining a second native GUI toolkit for Windows (or a cross-platform GUI framework) would roughly double UI effort and introduce a dependency not required by the core product goal.

## Decision

macOS receives a platform-native menu bar UX (AppKit + SwiftUI). Windows remains CLI + daemon only in this scope; no tray icon or window.

## Consequences

Windows users interact entirely through `lcp`. Any future Windows GUI is new scope, not a gap in this release, and must preserve existing CLI semantics and the network protocol (see [[0003-iroh-instead-of-lan-tcp]] and the protocol versioning rules).
