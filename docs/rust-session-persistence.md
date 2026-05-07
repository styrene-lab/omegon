+++
id = "2d809756-7b5e-44dd-b31a-d9c34747c965"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Rust session persistence — save/load conversation state, session resume

## Overview

The Rust binary currently runs one-shot (cleave children). For interactive sessions, it needs:
- Save conversation state to disk on exit (JSON serialization of ConversationState + IntentDocument)
- Load and resume a previous session on startup
- Session listing and selection
- Episode generation on session end (via LLM bridge call)

Currently pi manages sessions in `~/.config/omegon/sessions/`. The Rust binary should read/write compatible formats or introduce its own session store.

## Research

### What exists vs. what's needed

**Already implemented:**
- `ConversationState::save_session(path)` — serialize to JSON (messages + intent + decay_window + compaction_summary)
- `ConversationState::load_session(path)` — deserialize and reconstruct canonical history
- Round-trip tested
- Currently only saves for cleave children (`.cleave-session.json` in worktree)

**What's needed for full session persistence:**
1. **Session directory management** — create `~/.config/omegon/sessions/<cwd-slug>/` directory structure
2. **Auto-save on exit** — always save after agent loop completes (not just cleave children)
3. **Session listing** — enumerate saved sessions for a given cwd, sorted by timestamp
4. **Session resume via CLI** — `omegon-agent --resume [session-id]` loads a previous session
5. **Session ID generation** — `<timestamp>_<short-id>.json` format

**What's NOT needed for Phase 1:**
- Episode generation (requires LLM call, can be added later)
- Session selection UI (TUI bridge concern)
- Session pruning/cleanup
- Cross-session search

## Decisions

### Decision: Session storage uses ~/.config/omegon/sessions/<cwd-slug>/<timestamp>_<short-id>.json — compatible with pi's directory structure

**Status:** decided
**Rationale:** Pi already uses this directory structure. Sharing it means the TS parent can see Rust session files and vice versa during the transition. The cwd slug replaces / with - and strips leading --. Session files use .json (our format) rather than .jsonl (pi's format) since the internal structure differs, but they coexist in the same directory.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `core/crates/omegon/src/session.rs` (new) — Session manager: directory layout, save-on-exit, list, resume, ID generation
- `core/crates/omegon/src/main.rs` (modified) — Add --resume CLI arg, wire session save on all exits, load on resume

### Constraints

- Session save must happen on both normal exit and Ctrl+C interruption
- --resume with no argument resumes the most recent session for the cwd
- --resume <id> resumes a specific session by its short ID or full filename
- Session files must include enough metadata to display in a list: cwd, timestamp, turn count, last user prompt snippet
