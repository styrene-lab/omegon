+++
id = "f8ef2806-bb0a-4457-93e1-5418c2de28c8"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Rust session persistence — save/load conversation state, session resume — Design Spec (extracted)

> Auto-extracted from docs/rust-session-persistence.md at decide-time.

## Decisions

### Session storage uses ~/.pi/agent/sessions/<cwd-slug>/<timestamp>_<short-id>.json — compatible with pi's directory structure (decided)

Pi already uses this directory structure. Sharing it means the TS parent can see Rust session files and vice versa during the transition. The cwd slug replaces / with - and strips leading --. Session files use .json (our format) rather than .jsonl (pi's format) since the internal structure differs, but they coexist in the same directory.

## Research Summary

### What exists vs. what's needed

**Already implemented:**
- `ConversationState::save_session(path)` — serialize to JSON (messages + intent + decay_window + compaction_summary)
- `ConversationState::load_session(path)` — deserialize and reconstruct canonical history
- Round-trip tested
- Currently only saves for cleave children (`.cleave-session.json` in worktree)

**What's needed for full session persistence:**
1. **Session directory management** — create `~/.pi/agent/sessions/<cwd-slug>/` directory structure
2. **Auto-save on …
