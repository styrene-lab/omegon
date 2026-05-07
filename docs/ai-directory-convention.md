+++
id = "cfbcdef5-8055-4f04-b672-d90a0109e4f2"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# ai/ directory convention — unified agent artifact home

## Overview

Adopt the emerging ai/ directory convention as the home for all agent-specific artifacts: design docs, OpenSpec changes, memory facts, lifecycle state. Currently scattered across docs/, openspec/, .omegon/memory/, .omegon/lifecycle/. The ai/ folder is visible, version-controlled, and semantically clear — it says 'this is agent-managed content' without hiding behind dotfiles.\n\nThe .omegon/ dotfile remains for tool configuration only (profile.json, tutorial state, calibration).\n\nWhen we encounter an existing project with an ai/ directory, we can enrich it with our more robust conventions (design tree, OpenSpec, memory, milestones).

## Decisions

### Decision: ai/ is the unified agent artifact home

**Status:** decided
**Rationale:** Current layout: design docs in docs/, OpenSpec in openspec/, memory in .omegon/memory/, lifecycle in .omegon/lifecycle/, milestones in .omegon/milestones.json. All scattered. The ai/ convention is emerging in the wild as the standard place for agent-managed content. Moving everything under ai/ makes it obvious what the agent touches, what's version-controlled agent work, and lets us enrich existing ai/ folders with our robust conventions. Layout: ai/docs/ (design tree), ai/openspec/ (lifecycle), ai/memory/ (facts), ai/lifecycle/ (opsx state), ai/milestones.json. The .omegon/ dotfile stays for tool config only: profile.json, tutorial state, calibration, agents/. AGENTS.md stays at repo root (it's a project convention file like .gitignore, not an agent artifact).

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `core/crates/omegon/src/lifecycle/context.rs` (modified) — Change docs_dir from repo/docs to repo/ai/docs (with fallback to repo/docs)
- `core/crates/omegon/src/lifecycle/design.rs` (modified) — No changes needed — scan functions take a dir parameter
- `core/crates/omegon/src/features/lifecycle.rs` (modified) — Change openspec path from repo/openspec to repo/ai/openspec (with fallback)
- `core/crates/omegon/src/setup.rs` (modified) — Change memory_dir from .omegon/memory to ai/memory (with fallback chain: ai/memory → .omegon/memory)
- `core/crates/omegon/src/tui/mod.rs` (modified) — Update first-run heuristic to check ai/memory/facts.jsonl
- `core/crates/omegon/src/features/harness_settings.rs` (modified) — Update memory_stats and sessions paths to ai/ convention
- `core/crates/omegon-git/src/repo.rs` (modified) — Add ai/ to lifecycle path classification
- `core/crates/opsx-core/src/store.rs` (modified) — Change state.json path from .omegon/lifecycle to ai/lifecycle (with fallback)
- `core/crates/omegon/src/migrate.rs` (modified) — Add pi memory/lifecycle migration to ai/, enhance /migrate pi to copy facts.jsonl, add project-level convention scanning
- `core/crates/omegon/src/tui/mod.rs` (modified) — Add /init slash command that scans for other agent conventions and offers migration
- `core/crates/omegon/src/settings.rs` (modified) — Update milestones path to ai/milestones.json

### Constraints

- Backward compat: every path resolver checks ai/ first, then old location (docs/, openspec/, .omegon/memory/, .omegon/lifecycle/), without `.pi` legacy fallbacks
- AGENTS.md stays at repo root — it's a project convention file, not an agent artifact
- .omegon/ stays for tool config: profile.json, tutorial_completed, calibration, agents/
- The ai/ directory should be created on first write, not eagerly on startup
- Existing projects with content in docs/ and openspec/ must continue to work without manual migration
