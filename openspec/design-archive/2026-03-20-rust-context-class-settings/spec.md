+++
id = "d8876857-fd4e-4341-95bc-ce61ffdc4afa"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Rust settings integration — context class, provider preference, and downgrade overrides — Design Spec (extracted)

> Auto-extracted from docs/rust-context-class-settings.md at decide-time.

## Decisions

### ContextClass replaces ContextMode as the primary context abstraction; ContextMode becomes a derived property (decided)

ContextMode (Standard/Extended) is a legacy Anthropic-specific toggle. ContextClass (Squad/Maniple/Clan/Legion) subsumes it — Legion implies extended capability, Squad/Maniple imply standard. The Anthropic beta header derivation moves to a method on ContextClass. ContextMode is kept as a deprecated alias for backward compatibility with existing profile.json files.

### Route matrix embedded at compile time via include_str!, not loaded from disk at runtime (decided)

The route matrix is a reviewed snapshot that ships with the binary. Embedding via include_str! guarantees availability without filesystem dependency, makes the binary self-contained, and matches the design constraint that runtime routing consumes only the last reviewed local snapshot. The TS side loads from disk because extensions are filesystem-resident, but the Rust binary should be hermetic.

### Profile persists provider_order, context_floor_pin, and downgrade_overrides alongside existing fields (decided)

Provider order and context floor are operator preferences that should survive across sessions. Downgrade overrides (the durable 'don't ask again' decisions) must persist or they have no value. All three are additive optional fields with skip_serializing_if, so old profiles remain compatible.

## Research Summary

### Current Rust settings architecture

settings.rs has two layers: (1) `Settings` — runtime-mutable, session-scoped, behind `Arc<Mutex<Settings>>`. Fields: model, thinking, max_turns, compaction_threshold, context_window, context_mode, tool_detail. (2) `Profile` — persists to `.omegon/profile.json` (project-level) or `~/.config/omegon/profile.json` (global). Fields: last_used_model, thinking_level, max_turns. Loaded in main.rs on startup, saved on model change. `ContextMode` is currently a binary Standard(200k)/Extended(1M) toggle sp…

### Changes needed for context class integration

1. Add `ContextClass` enum (Squad/Maniple/Clan/Legion) to settings.rs with serde, ordinal, display, and classification from token count. 2. Replace `ContextMode` (binary) with `ContextClass` — the class subsumes the mode (Legion implies extended context capability). Keep ContextMode for backward compat but derive it from ContextClass. 3. Add `ThinkingLevel::Minimal` variant to match TS parity — currently 4 levels, TS has 5. 4. Add display labels: thinking levels → Servitor/Functionary/Adept/Mago…
