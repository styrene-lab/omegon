+++
id = "40cc0a8d-639d-49a7-a1f1-7b24b7e0ea18"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Phase 0 — Headless Rust agent loop as cleave child executor

## Overview

**SHIPPED in 0.11.0.** The Rust omegon-agent binary runs as the cleave child executor. 9.4k LoC, 118 tests, 3.4MB binary. Includes: agent loop state machine, LLM bridge subprocess (ndjson over stdio), 4 core tools (bash, read, write, edit), 8 memory tools (JSONL import on startup), NDJSON progress events on stdout, cleave orchestrator (worktree management, wave dispatch, merge), commit-nudge, auto-commit, guardrails, and test directives. The TS native-dispatch.ts wrapper parses progress events and maps them to dashboard state.

## Open Questions

*No open questions.*
