---
name: ratatui-tui
description: Project-resolved Rust terminal UI engineering with Ratatui, Crossterm, TachyonFX, companion crates, terminal ownership, async fairness, and TDD
aliases: [ratatui, rust-tui, tui-dev]
tags: [rust, tui, ratatui, terminal]
activation: project_detected
profile: [coding]
project_signals: [Cargo.toml, "**/tui/**/*.rs", "**/terminal*.rs"]
---

# Ratatui / Rust TUI Engineering

Use this skill for Rust terminal interfaces built on Ratatui. The repository is authoritative: do not assume crate versions, enabled features, viewport behavior, or APIs from memory.

## Resolve the Active Stack First

From the workspace root, run:

```bash
python3 skills/ratatui-tui/scripts/resolve_stack.py
```

The resolver reads Cargo metadata and lock resolution. It emits declared requirements and features, exact resolved versions, `tui` feature membership, local source paths, and version-pinned docs.rs links. If the script is unavailable in an installed skill, use `cargo metadata --format-version 1` directly; never copy the versions shown by an older session or this document.

Treat crates enabled by the package's `tui` feature as the active stack. Direct TUI dependencies remain visible even if feature metadata is absent. Candidate libraries in `references/ecosystem-candidates.md` are research leads, not dependencies or recommendations.

## Engineering Contract

### Terminal ownership is explicit

- Name which component owns the primary screen, alternate screen, cursor, mouse capture, raw mode, paste mode, and keyboard enhancement.
- Track every acquired terminal mode symmetrically. Update tracked state only after the terminal command succeeds.
- A panic, early return, partial transition, and ordinary quit must restore exactly the modes owned.
- `Frame::area()` is the only drawable rectangle. Never infer ownership of shell history or cells outside it.
- Inline, fixed, and fullscreen viewports have different lifecycle semantics. Read the resolved Ratatui source before changing viewport behavior.

### Rendering is complete and bounded

- Render every owned cell each frame; clear exclusive surfaces before drawing them.
- Keep semantic projection separate from Ratatui geometry.
- Use stable item identities, not numeric indices, when restoring selection after refresh or resize.
- Derive paging from actual visible list capacity. Page by capacity minus one row for continuity.
- Bound external text, queue sizes, retained notifications, syntax-highlighting input, and animation work.

### The event loop remains fair

- Agent/runtime work must not pause because a menu, prompt, or inspector is visible.
- Bound input and producer drains by count and/or wall time.
- Rendering is presentation-only: never await agent completion to change surfaces.
- Coalesce background redraws; keep cancellation and operator input latency-sensitive.
- Do not use inherited child-process stdio inside a TUI.

### Transcript and native scrollback are transactional

- Canonical conversation state is the source of truth.
- Do not publish primary-screen history while alternate-screen content is active.
- Inspect eligible records without committing the cursor, perform insertion, then commit after confirmed delivery. A known failure before writing can retry; ambiguous partial delivery must not blindly replay records.
- Never duplicate canonical records into an unbounded presentation queue.

### Stateful widgets own explicit state

- Keep `ListState`, selection identity, filter, tab, and layout capacity coherent after every transition.
- Recompute visual offset when geometry changes; reveal the semantic selection.
- Child selectors/prompts require explicit return targets. Input must not fall through to the composer.

## TachyonFX

TachyonFX is post-render presentation, not lifecycle state.

- Apply effects only to explicit owned rectangles after semantic layout is rendered.
- Rebind or clear effect regions after resize or terminal-ownership changes.
- Animation timers may mark frames dirty but must obey frame budgets and degrade cleanly.
- Never obscure approvals, errors, the cursor, or accessibility-critical text.
- Test stable start/end buffers and clipping independently of wall-clock timing.
- Confirm resolved APIs and enabled features with the resolver before editing effects.

## Companion Stack Review

For each resolved active crate, inspect its purpose and local source before use. Common concerns:

- `ratatui-image`: protocol ownership, skipped cells, erase/resize behavior, textual transcript fallback.
- `ratatui-textarea`: logical versus visual rows, wrapped cursor geometry, paste, draft preservation.
- `ratatui-toaster`: bounded/deduplicated passive queues and suppression during exclusive ownership.
- `tui-tree-widget`: stable identities, expansion state, selection restoration.
- `tui-popup`: reserve for genuinely compact interactions; do not use it to fake full ownership.
- `ansi-to-tui` / syntax crates: sanitize control sequences, cap input, cache expensive work, preserve cell widths.
- `hyperrat`: establish actual project usage; do not infer behavior from its name.

## TDD Ladder

Write state-machine and buffer tests before terminal wiring:

1. Pure transition tests: ownership, nesting, return targets, rollback.
2. Widget/state tests: selection identity, filtering, resize, page capacity.
3. Buffer tests with `TestBackend`: every owned cell, constrained dimensions, clipping.
4. Scheduler tests: events continue while exclusive surfaces render.
5. Publication tests: defer, retry, exactly-once order.
6. Guard tests: partial acquisition, release, panic/drop idempotence.
7. Focused interactive/PTY probe only where backend behavior cannot be proven in memory.
8. Repository-native crate tests and clippy before landing.

Do not make timing-only snapshot tests. Control clocks or assert stable endpoints.

## Project Evidence

Before changing architecture, search for local terminal guards, render schedulers, viewport probes, publication cursors, and surface projections. Project invariants override generic examples. Use repository-owned validation commands rather than assuming raw Cargo commands are sufficient.

## Further Guidance

- `references/architecture.md`
- `references/ecosystem-candidates.md`

## Safety Notes

The resolver executes `cargo metadata` as an argument-array subprocess without a shell and does not fetch documentation. It uses a 60-second timeout and rejects output larger than 32 MiB after capture; capture memory is not bounded by that check. Set `CARGO_NET_OFFLINE=true` when dependency downloads are not permitted. Generated URLs are references, not proof that an API exists; local resolved source is stronger evidence.
