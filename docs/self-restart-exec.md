---
id: self-restart-exec
title: Self-restart via exec() — agent rebuilds and hot-replaces its own binary
status: deferred
parent: rust-agent-loop
tags: [rust, self-update, exec, autonomy, lifecycle]
open_questions:
  - How do we handle exec() on non-Unix (Windows)? OpenCrabs just errors — is that acceptable or do we need a spawn-and-exit fallback?
  - "Should /evolve pull from GitHub Releases or cargo install? We already have a release pipeline (brutus) — downloading the binary and swapping is faster than compiling from source."
issue_type: feature
priority: 2
---

# Self-restart via exec() — agent rebuilds and hot-replaces its own binary

## Overview

The agent should be able to modify its own source, rebuild, health-check the new binary, and exec()-replace itself — resuming the same session seamlessly. Inspired by OpenCrabs' SelfUpdater pattern: cargo build --release → verify new binary (--version with timeout) → backup current → atomic rename → exec() with session ID passthrough. On failure, automatic rollback to backup. Three modes: (1) /rebuild — build from local source, (2) /evolve — download latest release from GitHub, swap binary, (3) agent-initiated — agent edits source, calls rebuild tool autonomously. The running binary is in memory so disk modifications are safe. Session continuity via env var or CLI arg pointing to active session. Already have rust-session-persistence (implemented) to support resume.

## Decisions

### Decision: Gated by permissions model, not freely agent-callable

**Status:** decided
**Rationale:** Self-restart is a privileged operation — the agent shouldn't freely rebuild and replace itself without operator consent. Should be integrated into the internal permissions model (like vault access restrictions). A settings toggle enables/disables the capability. When disabled, the rebuild/evolve tools are not registered. When enabled, they still require operator approval before exec().

## Open Questions

- How do we handle exec() on non-Unix (Windows)? OpenCrabs just errors — is that acceptable or do we need a spawn-and-exit fallback?
- Should /evolve pull from GitHub Releases or cargo install? We already have a release pipeline (brutus) — downloading the binary and swapping is faster than compiling from source.
