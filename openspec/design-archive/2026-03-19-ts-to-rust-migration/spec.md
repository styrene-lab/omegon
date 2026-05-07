+++
id = "5ecabe99-3f6e-4c01-abd3-5441ca4d80f0"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# TS→Rust Migration: Make omegon repo Rust-primary — Design Spec (extracted)

> Auto-extracted from docs/ts-to-rust-migration.md at decide-time.

## Decisions

### Three-phase migration with secrets and tool wiring first (decided)

Phase 1 (secrets, wire existing tools, memory tools, whoami) is the minimum for the Rust binary to be self-sufficient. Phase 2 (design tree, openspec, cleave assessment) completes agent lifecycle. Phase 3 (render, mcp, igor) stays external or dropped. TS layer archives after Phase 1.

## Research Summary

### Parity Audit: Rust vs TS Extensions

**Rust tools registered:** bash, read, write, edit, chronos, change, speculate_*
**TS tools NOT in Rust:** whoami, view, web_search, ask_local_model, list_local_models, manage_ollama, generate_image_local, render_diagram, render_native_diagram, render_excalidraw, render_composition_still, render_composition_video, memory_* (7 tools), design_tree, design_tree_update, openspec_manage, cleave_assess, cleave_run, manage_tools, set_model_tier, set_thinking_level, switch_to_offline_driver, execute_sla…

### Revised Parity Audit — Rust is 90%+ complete

setup.rs already registers: CoreTools (bash/read/write/edit/change/speculate/chronos), WebSearchProvider, LocalInferenceProvider, ViewProvider, RenderProvider, MemoryProvider (8 memory tools), LifecycleFeature (design_tree, design_tree_update, openspec_manage), CleaveFeature (cleave_assess, cleave_run), ModelBudget (set_model_tier, set_thinking_level), AutoCompact, SessionLog, VersionCheck, TerminalTitle.

Actually missing: whoami, manage_tools, switch_to_offline_driver, memory_compact, memory_s…
