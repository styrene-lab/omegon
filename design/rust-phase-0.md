+++
id = "cb612eb3-be81-4868-8e2f-0d5686fa9a2f"
kind = "design_node"
title = "Phase 0 — Headless Rust agent loop as cleave child executor"
status = "implemented"
tags = ["rust", "phase-0", "cleave", "headless"]
aliases = ["rust-phase-0"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
parent = "rust-agent-loop"
+++

# Phase 0 — Headless Rust agent loop as cleave child executor

## Overview

**SHIPPED in 0.11.0.** The Rust omegon-agent binary runs as the cleave child executor. 9.4k LoC, 118 tests, 3.4MB binary. Includes: agent loop state machine, LLM bridge subprocess (ndjson over stdio), 4 core tools (bash, read, write, edit), 8 memory tools (JSONL import on startup), NDJSON progress events on stdout, cleave orchestrator (worktree management, wave dispatch, merge), commit-nudge, auto-commit, guardrails, and test directives. The TS native-dispatch.ts wrapper parses progress events and maps them to dashboard state.

## Open Questions

*No open questions.*
