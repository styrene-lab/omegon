---
id: rust-crate-consolidation
title: Rust crate consolidation and ecosystem leverage
status: implemented
parent: ts-to-rust-migration
open_questions: []
---

# Rust crate consolidation and ecosystem leverage

## Overview

Audit all Rust crates/features for consolidation, redundancy, and opportunities to leverage the Rust ecosystem (e.g., keyring crate for secrets, better redaction, structured tool guards).

## Research

### Crate architecture audit

**Current state: 4 crates, ~33K LOC total**

| Crate | LOC | Purpose |
|-------|-----|---------|
| omegon | 28,679 | Main binary — everything |
| omegon-memory | 3,585 | Memory backend + 12 tools |
| omegon-secrets | 734 | Redaction, guards, recipes, audit |
| omegon-traits | 382 | ToolDefinition, ToolResult, ContentBlock |

**omegon main crate breakdown:**
- TUI: 7,116 LOC (25%) — ratatui widgets, dashboard, splash, editor
- Tools: 4,222 LOC (15%) — 14 tool files
- Features: 2,935 LOC (10%) — 9 features
- Lifecycle: 2,573 LOC (9%) — design tree, openspec
- Cleave: 1,843 LOC (6%) — orchestrator, worktrees
- Agent loop: 1,307 LOC — main dispatch
- Conversation: 1,424 LOC — message management
- Providers: 730 LOC — Anthropic/OpenAI
- Web: 880 LOC — axum API + websocket
- Auth: 548 LOC — OAuth flow
- Other: ~5K LOC — main, setup, context, prompt, bridge, migrate, session, settings

### Ecosystem leverage opportunities

**1. omegon-secrets → keyring + secrecy + aho-corasick**
Current: hand-rolled keychain shell-out, simple string::replace redaction, plaintext secret values in HashMap.
Upgrade path:
- `keyring` (v3) — cross-platform credential store (macOS Keychain, Windows Credential Manager, Linux Secret Service). Replaces the `security find-generic-password` shell-out in resolve.rs. Adds Windows/Linux support for free.
- `secrecy` + `zeroize` — wrap secret values in `Secret<String>` that auto-zeroes on drop. Prevents secrets from lingering in memory after the session ends. `ExposeSecret` trait forces explicit access.
- `aho-corasick` — single-pass multi-pattern replacement for redaction. Current impl does N sequential `str::replace` calls (one per secret). For small N (<20 secrets), str::replace is fine — but Aho-Corasick is the correct algorithm and prevents quadratic behavior as secrets grow.

**2. omegon main crate — candidate splits**
The 28K LOC monolith has clear seam lines:
- `omegon-tui` — 7K LOC TUI could be its own crate. Depends on ratatui, crossterm, tachyonfx. Currently tightly coupled to agent state via dashboard handles.
- `omegon-providers` — 730 LOC + 548 auth + 520 bridge. Provider/auth layer could be factored out for reuse by other Rust agent projects.
- `omegon-lifecycle` — 2.5K lifecycle + 1.8K cleave + 972 lifecycle feature. Design tree, openspec, cleave as a standalone crate.
- Counter-argument: splitting for splitting's sake adds build complexity. The monolith compiles in ~6s incremental. Only split when there's a reuse or testability benefit.

**3. Tool consolidation**
- `speculate_*` (4 tools, 363 LOC) — git-based speculation. Could be folded into the `change` tool as a mode flag instead of 4 separate tools.
- `change` + `edit` — change is "atomic multi-edit + validation", edit is single edit. Could merge, but the simple edit interface is useful for the LLM.
- Model tier aliases (gloriana/victory/retribution/haiku/sonnet/opus) — these are command aliases, not tools. Clean up the confusion by removing them from tools() and keeping them only in commands().

**4. Feature consolidation**
- `auto_compact` (144 LOC) — fires context compaction. Could be a method on ContextManager instead of a full Feature.
- `terminal_title` (137 LOC) — sets terminal title from session state. Very thin — could be a bus event handler in TUI.
- `session_log` (216 LOC) — injects recent session log into context. Could be part of ContextManager.
- These three are <500 LOC combined and only implement on_event + provide_context. They don't provide tools or commands. Merging them into a `NativeFeatures` or just into the agent loop would reduce bus registration overhead.

**5. Memory crate**
- Already well-factored. 12 tools, sqlite backend, vector search.
- Could add `sled` or `redb` as an alternative backend for zero-config embedded use (no sqlite compilation).
- Vector search currently uses custom cosine similarity — could leverage `usearch` or `lance` for HNSW when the fact count grows.

## Decisions

### Decision: Secrets crate upgraded to keyring+secrecy+aho-corasick; thin features kept separate

**Status:** decided
**Rationale:** The secrets crate got the highest-value ecosystem upgrades: keyring for cross-platform credential storage (macOS+Linux+Windows), secrecy+zeroize for memory-safe secret handling, aho-corasick for single-pass redaction. The thin features (auto_compact, terminal_title, session_log) are well-structured with tests and not worth merging — clarity beats marginal registration savings. The omegon main crate is a 28K monolith but compiles fast (~6s) and has clear internal module boundaries — splitting would add complexity without clear reuse benefit yet.

## Open Questions

*No open questions.*
