---
id: rust-context-class-settings
title: Rust settings integration — context class, provider preference, and downgrade overrides
status: implemented
parent: context-class-taxonomy-and-routing-policy
tags: [rust, settings, routing, persistence, context-class]
open_questions: []
---

# Rust settings integration — context class, provider preference, and downgrade overrides

## Overview

Wire the context class taxonomy into the Rust Settings/Profile persistence layer. The TS side has the runtime logic (context-class.ts, route-envelope.ts, routing-state.ts, downgrade-policy.ts). The Rust side needs: ContextClass enum, provider preference persistence in Profile, downgrade override storage, ThinkingLevel parity (add Minimal), replace hardcoded infer_context_window with route matrix, and dashboard display of context class.

## Research

### Current Rust settings architecture

settings.rs has two layers: (1) `Settings` — runtime-mutable, session-scoped, behind `Arc<Mutex<Settings>>`. Fields: model, thinking, max_turns, compaction_threshold, context_window, context_mode, tool_detail. (2) `Profile` — persists to `.omegon/profile.json` (project-level) or `~/.config/omegon/profile.json` (global). Fields: last_used_model, thinking_level, max_turns. Loaded in main.rs on startup, saved on model change. `ContextMode` is currently a binary Standard(200k)/Extended(1M) toggle specific to Anthropic beta headers. `ThinkingLevel` has 4 values (Off/Low/Medium/High) — TS has 5 (adds Minimal). `infer_context_window` is a hardcoded heuristic using string matching on model names. FooterData in tui/footer.rs reads context_window and context_mode from Settings for display.

### Changes needed for context class integration

1. Add `ContextClass` enum (Squad/Maniple/Clan/Legion) to settings.rs with serde, ordinal, display, and classification from token count. 2. Replace `ContextMode` (binary) with `ContextClass` — the class subsumes the mode (Legion implies extended context capability). Keep ContextMode for backward compat but derive it from ContextClass. 3. Add `ThinkingLevel::Minimal` variant to match TS parity — currently 4 levels, TS has 5. 4. Add display labels: thinking levels → Servitor/Functionary/Adept/Magos/Archmagos. 5. Add to Profile: `provider_order`, `avoid_providers`, `context_floor_pin`, `downgrade_overrides` (vec of accepted downgrades). 6. Replace `infer_context_window` with route matrix lookup — load data/route-matrix.json or embed at compile time. 7. Add `context_class` field to Settings (derived from context_window). 8. Add `provider_preference` field to Settings for runtime ordering. 9. Update FooterData to include context_class for dashboard display. 10. Update TUI footer rendering to show context class badge.

## Decisions

### Decision: ContextClass replaces ContextMode as the primary context abstraction; ContextMode becomes a derived property

**Status:** decided
**Rationale:** ContextMode (Standard/Extended) is a legacy Anthropic-specific toggle. ContextClass (Squad/Maniple/Clan/Legion) subsumes it — Legion implies extended capability, Squad/Maniple imply standard. The Anthropic beta header derivation moves to a method on ContextClass. ContextMode is kept as a deprecated alias for backward compatibility with existing profile.json files.

### Decision: Route matrix embedded at compile time via include_str!, not loaded from disk at runtime

**Status:** decided
**Rationale:** The route matrix is a reviewed snapshot that ships with the binary. Embedding via include_str! guarantees availability without filesystem dependency, makes the binary self-contained, and matches the design constraint that runtime routing consumes only the last reviewed local snapshot. The TS side loads from disk because extensions are filesystem-resident, but the Rust binary should be hermetic.

### Decision: Profile persists provider_order, context_floor_pin, and downgrade_overrides alongside existing fields

**Status:** decided
**Rationale:** Provider order and context floor are operator preferences that should survive across sessions. Downgrade overrides (the durable 'don't ask again' decisions) must persist or they have no value. All three are additive optional fields with skip_serializing_if, so old profiles remain compatible.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `core/crates/omegon/src/settings.rs` (modified) — Add ContextClass enum, ThinkingLevel::Minimal, derive ContextMode from ContextClass, replace infer_context_window with route matrix lookup, add context_class to Settings, add provider_preference/context_floor_pin/downgrade_overrides to Profile
- `core/crates/omegon/src/tui/footer.rs` (modified) — Add context_class to FooterData, render context class badge in footer
- `core/crates/omegon/src/tui/dashboard.rs` (modified) — Add context_class display to dashboard model section
- `core/crates/omegon/src/main.rs` (modified) — Wire context_class derivation on startup and model change

### Constraints

- ContextClass must mirror the TS enum exactly: Squad (128k), Maniple (272k), Clan (400k), Legion (1M+)
- ThinkingLevel must add Minimal to match TS parity (5 levels)
- Profile fields are additive and optional — old profiles deserialize cleanly
- Route matrix is compile-time embedded from the same JSON the TS side loads from disk
- ContextMode remains for Anthropic beta header derivation but is deprecated as operator-facing
- Display labels use Mechanicum names: thinking → Servitor/Functionary/Adept/Magos/Archmagos
- Downgrade overrides in profile are a Vec of accepted transitions, not a blanket disable
