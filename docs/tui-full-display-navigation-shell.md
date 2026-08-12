---
id: tui-full-display-navigation-shell
title: "Full-display TUI navigation shell"
status: decided
parent: tui-surface-substrate-reevaluation
tags: [tui, navigation, settings, inline-terminal, menus, ux]
open_questions: []
dependencies: []
related: []
---

# Full-display TUI navigation shell

## Overview

Replace constrained conversation overlays with an exclusive full-display interactive surface model. `/settings` becomes the canonical configuration hierarchy; task-oriented commands such as `/extensions`, `/skills`, `/model`, and `/auth` deep-link into settings destinations. Inspectors and blocking prompts use the same exclusive display ownership without being misclassified as settings. Semantic menu projections remain renderer-neutral; the TUI shell owns navigation history, input routing, viewport transitions, and return to conversation.

## Research

### Current inline viewport and menu geometry

Current inline mode creates `Viewport::Inline(native_transcript_viewport_height(terminal_height))`, where the compact viewport is capped at eight rows (`core/crates/omegon/src/tui/mod.rs`). Structured menus then call `command_modal_area(area)`, which subtracts margins and caps the result at 120×32 (`core/crates/omegon/src/tui/menu_surface.rs`, `command_surfaces.rs`). Menu row windowing cannot solve the resulting loss of usable space because conversation chrome and modal margins compete for the same compact physical viewport.

### Existing universal-settings seam

The existing settings projection already declares itself the universal configuration entrypoint and projects Runtime, Model, Authentication, Skills, Extensions, UI, Context, Memory, Profiles, Secrets, Sandbox, and Updates as configuration areas (`core/crates/omegon/src/tui/settings_menu_projection.rs`). Those rows currently execute canonical slash commands, replacing `active_menu` with independent menu projections. Existing tests cover `/settings auth` and `/settings skills` direct routing and settings rows targeting `/extension`, `/skills`, `/auth`, and `/model` (`core/crates/omegon/src/tui/tests.rs`). The implementation therefore has the semantic destinations but lacks a persistent navigation stack and shared shell.

### Interactive surface inventory and state fragmentation

Current interactive states are fragmented across `command_prompt`, `selector`, `active_menu`, `process_viewer`, `menu_input`, `at_picker`, copy-text state, and extension-provided modals in `App` (`core/crates/omegon/src/tui/mod.rs`). `execute_active_menu_action` currently clears `active_menu` before opening selectors, losing parent navigation context (`core/crates/omegon/src/tui/menu_effects.rs`). Process and copy viewers are detail pages in behavior despite modal geometry. Confirmation already uses second activation plus a warning, demonstrating that modal lifecycle semantics do not require visual overlay composition.

### Operator validation: live terminal resizing

Operator validation on 2026-08-12: resized the active inline terminal repeatedly, retained normal scrolling, and observed all permanent TUI elements react correctly to changing geometry. This validates terminal resize handling and scrollback preservation under geometry changes. The implementation still needs to express compact-to-full-display transitions deliberately, but terminal geometry change itself is not an architectural blocker.

## Decisions

### Interactive surfaces exclusively own the live inline display

**Status:** accepted

**Rationale:** The compact conversation viewport cannot simultaneously provide useful menu capacity. When navigation, an inspector, a picker, or a blocking prompt is active, normal conversation composition and permanent fixtures are suspended. Modal describes input/lifecycle semantics only; it does not imply visual layering over conversation.

### Settings is the canonical configuration hierarchy

**Status:** accepted

**Rationale:** Runtime, inference, authentication, skills, extensions, UI, context, memory, profiles, secrets, sandbox, and updates are configuration destinations under one settings navigation root. Existing independent semantic projection builders remain destination owners rather than being flattened into one monolithic projection.

### Task-oriented slash commands deep-link to canonical destinations

**Status:** accepted

**Rationale:** `/extensions`, `/skills`, `/model`, `/auth`, and equivalent commands remain ergonomic operator entrypoints but resolve to the same settings destination identities as `/settings <section>`. They must not create parallel menu state or duplicate command registries.

### Navigation stores destination identities and local state, not stale projections

**Status:** accepted

**Rationale:** A stack of destination identities provides breadcrumb and Back semantics while each destination rebuilds its renderer-neutral `MenuProjection` from current runtime state. This preserves independent refresh lifecycles and avoids retaining stale extension, skill, model, or secret inventories.

### Inspectors and blocking prompts share the shell but remain outside Settings

**Status:** accepted

**Rationale:** Process/session detail, copy/document viewers, help, tool evidence, and responder-backed decisions are not configuration. They use exclusive full-display ownership and common navigation/input infrastructure, but retain inspector or prompt semantics rather than being forced into the settings hierarchy.

### Back and Escape unwind the nearest interaction scope

**Status:** accepted

**Rationale:** Escape first cancels active search, inline value entry, or pending confirmation; otherwise it pops one child destination; at the root it closes the interactive surface and restores conversation. Selection completion returns to the exact parent destination unless the action explicitly navigates elsewhere.

### Bare settings opens the root; deep links are deterministic

**Status:** accepted

**Rationale:** `/settings` must be predictable and teach the hierarchy, so it opens the root rather than hidden session history. Explicit aliases and `/settings <section>` always open their named destination. A future resume affordance may be added explicitly instead of changing command semantics.

### First slice adapts legacy extension modals behind the exclusive surface boundary

**Status:** accepted

**Rationale:** Requiring every extension payload to adopt a typed page contract would make the initial migration unnecessarily broad. Existing extension modal payloads may render through a compatibility destination that exclusively owns the display; new extension UI should use typed menu, inspector, picker, or prompt projections. Compatibility does not permit rendering over conversation.

### Destination projection builders remain independent

**Status:** accepted

**Rationale:** The shell addresses and composes destinations; it does not absorb their backing state. This preserves existing renderer-neutral menu projections and allows extensions, skills, models, and secrets to refresh independently without a monolithic settings snapshot.

### Full-display transitions may rely on supported terminal geometry changes

**Status:** accepted

**Rationale:** Live operator validation showed repeated inline-terminal resizing preserves scrolling and correctly reflows permanent TUI elements. The navigation shell may therefore transition between compact conversation and full-display geometry using the existing resize-aware terminal path, with regression tests guarding restoration and non-republication.

## Implementation Notes

### File Scope

- `core/crates/omegon/src/surfaces/navigation.rs` — Renderer-neutral destination identities, navigation stack projection, breadcrumb, and transient interaction contracts. (create)
- `core/crates/omegon/src/surfaces/mod.rs` — Expose the navigation semantic surface module. (modify)
- `core/crates/omegon/src/tui/navigation.rs` — TUI navigation session state, canonical destination routing, stack transitions, and destination projection dispatch. (create)
- `core/crates/omegon/src/tui/mod.rs` — Replace `active_menu`-centric top-level interaction state with an exclusive interactive-surface owner and route slash aliases into canonical destinations. (modify)
- `core/crates/omegon/src/tui/render.rs` — Short-circuit normal conversation layout while an exclusive interactive surface is active; render the navigation shell across the entire current frame. (modify)
- `core/crates/omegon/src/tui/menu_surface.rs` — Render menu content into the supplied full-display content area rather than always deriving centered modal geometry. (modify)
- `core/crates/omegon/src/tui/settings_menu_projection.rs` — Convert settings area actions from recursive slash-command execution to canonical destination navigation effects. (modify)
- `core/crates/omegon/src/tui/menu_effects.rs` — Represent navigation, child selectors, inline input, and Back transitions as explicit effects that retain parent context. (modify)
- `core/crates/omegon/src/tui/process_viewer.rs` — Move process/detail viewing from modal geometry to the full-display inspector path. (modify)
- `core/crates/omegon/src/tui/tests.rs` — Add regression coverage for display ownership, deep links, navigation stack behavior, and conversation restoration. (modify)
- `core/crates/omegon/src/tui/snapshot_tests.rs` — Add end-to-end snapshots for full-display navigation at normal and constrained terminal dimensions. (modify)

### Constraints

- Do not special-case `/extensions` in the renderer; all structured interactive destinations use the same exclusive-display mechanism.
- Preserve renderer-neutral projections under `surfaces::*`; no Ratatui geometry or key codes may enter semantic projection types.
- Do not flatten settings inventories into a single eagerly-built projection; destination projections retain independent refresh and capability ownership.
- Entering and leaving full-display mode must preserve native terminal scrollback and restore the compact conversation viewport without republishing completed exchanges.
- Selectors opened from a settings destination must return to that destination on confirm or cancel rather than discarding parent context.
- Slash aliases and settings rows must resolve through a canonical destination router, not recursively execute UI slash commands.
- Tests must cover constrained terminal heights, viewport transition restoration, deep-link equivalence, breadcrumb/Back behavior, and input precedence.
