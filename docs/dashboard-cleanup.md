---
id: dashboard-cleanup
title: Dashboard cleanup and footer coherence
status: implementing
related: [dash-raised-layout, unified-dashboard]
tags: [dashboard, footer, ux, layout, cleanup]
open_questions: []
branches: ["feature/dashboard-cleanup"]
openspec_change: dashboard-cleanup
---

# Dashboard cleanup and footer coherence

## Overview

Clean up the unified dashboard so persistent operator metadata remains visible, duplicate footer behavior is eliminated, section spacing is consistent, and low-value status surfaces like recovery only consume space when they provide actionable information.

## Research

### Raised footer metadata should be operator-persistent

The operator explicitly wants driver/model/thinking level and memory visibility to remain pinned at the bottom of the dashboard when the raised view expands. This implies the raised layout should reserve its bottom rows for operator metadata and let upper dashboard sections absorb truncation or compression pressure instead of allowing the meta block to scroll out of view.

### OpenSpec spacing and duplicate footer symptoms

The current dashboard still shows visual rough edges: OpenSpec section spacing feels off, and the operator reports seeing what looks like both the custom dashboard context gauge and a second lower footer/status line that should have been subsumed already. That suggests the cleanup should audit the custom footer render path, extension status/footer data passthrough, and any remaining inherited pi footer/status surfaces that still consume vertical space.

### Recovery visibility should be conditional on actionability

Recovery state is useful during live incident handling, but the operator does not want it consuming multiple rows when it is only background telemetry. The cleanup should distinguish actionable recovery information from passive status so recovery only expands when it requires attention or offers a meaningful operator decision.

### Duplicate footer root cause from the compaction line

Inspection of `extensions/dashboard/footer.ts` shows the apparent duplicate footer is currently self-inflicted by the custom dashboard. In raised mode, `renderRaisedWide()` / `buildSharedFooterZone()` append `renderFooterData(width)`, and `renderFooterData()` intentionally re-renders a pwd line, a `57%/272k` style compaction/context line, and raised-mode extension statuses. The screenshot matches that exactly: the top dashboard meta line already shows the context gauge and driver/model, then the lower block repeats context and memory-oriented footer data after `/dash to compact`. So the extra line is not pi's original footer leaking through; it is the custom footer rendering both dashboard metadata and a preserved footer-data block.

### OpenSpec spacing and recovery structure audit

`extensions/dashboard/footer.ts` shows the wide raised layout currently renders Zone C as `Recovery + Cleave` in the left column and `OpenSpec` in the right column, then appends a full footer zone afterward. `buildOpenSpecLines()` uses a header plus up to three change rows with double-space separators and stage/progress suffixes, so any spacing awkwardness is likely within this renderer rather than from another footer layer. `buildRecoveryLines()` always consumes two rows whenever recovery state exists: a labeled status row and a summary/detail row. That means even low-value or already-resolved recovery telemetry permanently steals two lines from the active raised layout.

## Decisions

### Decision: Treat these nitpicks as one dashboard cleanup slice rather than isolated tweaks

**Status:** decided
**Rationale:** The pinned metadata issue, OpenSpec spacing roughness, duplicate footer impression, and over-expanded recovery area all stem from the same footer/layout coherence problem. Solving them together should produce a cleaner, more intentional dashboard instead of stacking point fixes.

### Decision: Raised dashboard should stop re-rendering redundant inherited footer data

**Status:** decided
**Rationale:** The current custom footer already renders context gauge, driver/model, and memory-oriented metadata in the dashboard-specific rows. Re-appending the generic footer-data block below `/dash to compact` creates the appearance of a duplicate footer and wastes vertical space without adding enough operator value.

### Decision: Reserve a fixed bottom metadata block in raised mode

**Status:** decided
**Rationale:** The operator depends on context gauge, driver/model, thinking level, memory summary, and the raise/lower hint as persistent controls. Raised mode should therefore reserve a small fixed bottom block for those rows and let the higher dashboard sections above absorb truncation instead of allowing the operator metadata to scroll away.

### Decision: Recovery expands only when it is actionable

**Status:** decided
**Rationale:** Recovery telemetry is useful, but in the steady state it should not consume two full rows of raised-dashboard space. The dashboard should collapse recovery into at most a compact badge or omit it entirely unless the current recovery state calls for operator awareness, such as escalation, active cooldown pressure, repeated retry exhaustion, or a meaningful model/provider switch decision.

### Decision: Raised mode should not append generic footer-data rows that duplicate dashboard metadata

**Status:** decided
**Rationale:** The audit shows `renderFooterData()` currently reintroduces a compact context line, pwd line, and raised-mode extension statuses after the dashboard has already rendered context gauge, model, and memory-oriented metadata. In raised mode those generic footer-data rows should be removed or reduced to only unique operator value, eliminating the visual impression of a second footer.

### Decision: Use tighter OpenSpec rows with fewer inline separators

**Status:** decided
**Rationale:** The OpenSpec right column should read as a compact status list, not a sentence. Prefer a tighter header, keep the change name visually primary, and reduce visual noise by minimizing repeated separators. Progress should remain concise and stage should be retained as a short suffix only when it adds differentiation.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/dashboard/footer.ts` (modified) — Restructure raised-mode layout so upper content yields to a fixed bottom metadata block, tighten OpenSpec row formatting, and collapse non-actionable recovery rendering.
- `extensions/dashboard/footer-raised.test.ts` (modified) — Add/update regression tests for pinned bottom metadata, non-duplicated raised footer output, OpenSpec spacing, and conditional recovery expansion.
- `extensions/dashboard/types.ts` (modified) — Adjust any dashboard-facing types needed for cleaner raised-mode section composition and recovery/actionability rendering.
- `extensions/dashboard/index.ts` (modified) — Only if needed for mode/render-state wiring after footer cleanup; avoid unrelated command-surface changes.
- `extensions/web-ui/state.ts` (modified) — Expose the same operator-facing dashboard metadata and recovery actionability structurally through backend snapshot builders.
- `extensions/web-ui/types.ts` (modified) — Extend dashboard/recovery snapshot types so web consumers can render the cleaned-up metadata coherently.
- `extensions/web-ui/index.test.ts` (modified) — Add/update backend/web UI state tests proving dashboard metadata parity with the TUI cleanup.

### Constraints

- Raised mode must keep a fixed bottom metadata block visible for context/model/thinking/memory and the compact hint.
- Raised mode must not append generic footer-data rows that duplicate dashboard metadata already rendered above.
- Recovery should expand only when actionable; otherwise collapse to a compact badge or disappear.
- OpenSpec rows should remain compact and visually scannable, with fewer inline separators and no wasted spacing.
- Dashboard cleanup metadata needed by the TUI footer should also be available structurally through the backend/web UI snapshot contract.
- Recovery actionability should be represented as structured state for web consumers rather than only as rendered footer text.
