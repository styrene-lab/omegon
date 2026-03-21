---
id: ts-to-rust-migration
title: "TS→Rust Migration: Make omegon repo Rust-primary"
status: implemented
tags: [migration, architecture, rust]
open_questions: []
---

# TS→Rust Migration: Make omegon repo Rust-primary

## Overview

Migrate the omegon repo from TS+pi-mono harness to Rust-primary. The Rust binary in core/ reimplements most functionality. Archive the TS/pi layer to a separate omegon-pi repo. Before migration, audit each TS extension to confirm Rust parity or intentional deprecation.

## Research

### Parity Audit: Rust vs TS Extensions

**Rust tools registered:** bash, read, write, edit, chronos, change, speculate_*
**TS tools NOT in Rust:** whoami, view, web_search, ask_local_model, list_local_models, manage_ollama, generate_image_local, render_diagram, render_native_diagram, render_excalidraw, render_composition_still, render_composition_video, memory_* (7 tools), design_tree, design_tree_update, openspec_manage, cleave_assess, cleave_run, manage_tools, set_model_tier, set_thinking_level, switch_to_offline_driver, execute_slash_command

**Rust has code for but not as registered tools:** web_search (tools/web_search.rs), view (tools/view.rs), render (tools/render.rs), local_inference (tools/local_inference.rs)

**Critical TS-only systems (no Rust equivalent):**
1. **00-secrets** (1239 LOC) — secret recipes, output redaction, tool guards, audit log. Rust has basic env-var key resolution only.
2. **01-auth/whoami** — multi-provider auth status (git, gh, aws, k8s, OCI). No Rust equivalent.
3. **project-memory** (30 files) — full memory lifecycle, semantic search, episodes, compaction. Rust omegon-memory crate exists for storage but not the tool/command layer.
4. **design-tree** (9 files) — tree queries/mutations. Rust lifecycle/design.rs exists but unclear completeness.
5. **openspec** (20 files) — spec lifecycle, verification. Rust lifecycle/spec.rs partial.
6. **cleave** (31 files) — assessment, review, bridge. Rust cleave/ has orchestrator but not assessment/review.
7. **dashboard** (21 files) — full TUI dashboard. Rust tui/ reimplements.
8. **mcp-bridge** — MCP server connectivity. Rust migrates config but doesn't bridge.
9. **igor** — Igor nervous system integration. No Rust equivalent.
10. **render/** — composition, excalidraw, native diagrams. These shell out to Node — may always need external process.

### Revised Parity Audit — Rust is 90%+ complete

setup.rs already registers: CoreTools (bash/read/write/edit/change/speculate/chronos), WebSearchProvider, LocalInferenceProvider, ViewProvider, RenderProvider, MemoryProvider (8 memory tools), LifecycleFeature (design_tree, design_tree_update, openspec_manage), CleaveFeature (cleave_assess, cleave_run), ModelBudget (set_model_tier, set_thinking_level), AutoCompact, SessionLog, VersionCheck, TerminalTitle.

Actually missing: whoami, manage_tools, switch_to_offline_driver, memory_compact, memory_search_archive, memory_episodes, memory_ingest_lifecycle, execute_slash_command, and the entire secrets/redaction system.

The tool wiring child node is already mostly done — only ~8 small tools remain. The secrets system is the real blocker.

## Decisions

### Decision: Three-phase migration with secrets and tool wiring first

**Status:** decided
**Rationale:** Phase 1 (secrets, wire existing tools, memory tools, whoami) is the minimum for the Rust binary to be self-sufficient. Phase 2 (design tree, openspec, cleave assessment) completes agent lifecycle. Phase 3 (render, mcp, igor) stays external or dropped. TS layer archives after Phase 1.

## Open Questions

*No open questions.*
