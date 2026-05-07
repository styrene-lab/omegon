+++
id = "06c376bf-26ed-40dc-b403-b625485d715e"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Dashboard cleanup and footer coherence — Design

## Spec-Derived Architecture

### dashboard/footer

- **Raised dashboard pins operator metadata at the bottom** (added) — 2 scenarios
- **Raised dashboard does not render a duplicate generic footer block** (added) — 2 scenarios
- **Recovery expands only when actionable** (added) — 2 scenarios
- **OpenSpec rows are compact and visually coherent** (added) — 2 scenarios

### web-ui/dashboard

- **Dashboard cleanup metadata is available to backend and web UI consumers** (added) — 2 scenarios

## Scope

Clean up the unified dashboard footer so raised mode behaves as a coherent operator panel rather than a stack of partially overlapping footer layers. The raised renderer should reserve a fixed bottom metadata block for operator-critical context, eliminate duplicate generic footer rows that repeat already-rendered dashboard information, collapse recovery unless it is actionable, tighten OpenSpec row formatting so the right column reads cleanly, and expose the same operator-facing metadata structurally through the backend/web UI snapshot contract.

## Design Approach

1. **Pinned bottom block in raised mode**
   - Refactor `extensions/dashboard/footer.ts` so raised mode explicitly separates:
     - upper dashboard sections (branch tree, design-tree, OpenSpec, cleave, actionable recovery)
     - lower pinned metadata block (context/model/thinking, memory summary, compact hint, and only any remaining unique operator metadata)
   - Upper sections should absorb truncation/compression before the bottom block disappears.

2. **Remove duplicate generic footer rows in raised mode**
   - Audit `renderFooterData()` and its callers.
   - In raised mode, do not append generic rows that duplicate the context gauge, model, memory, or other dashboard metadata already rendered in the pinned block.
   - Keep only uniquely useful operator data if anything truly remains after de-duplication.

3. **Conditional recovery expansion**
   - Introduce a clear rule in the raised footer renderer for when recovery gets the full expanded section.
   - Non-actionable or already-resolved recovery should collapse to a compact badge or disappear.
   - Actionable states such as escalation, active cooldown pressure, retry exhaustion, or important provider/model switch events should still surface clearly.

4. **OpenSpec formatting cleanup**
   - Tighten `buildOpenSpecLines()` so the OpenSpec header and per-change rows use less padding and fewer inline separators.
   - Keep the change name visually primary and preserve concise progress/stage cues only when they improve scanability.

5. **Backend/web UI metadata parity**
   - Extend the backend/web UI dashboard snapshot shape so it exposes the same pinned operator metadata inputs the TUI dashboard relies on.
   - Make recovery actionability available structurally so web consumers can distinguish actionable from non-actionable recovery without parsing rendered footer text.

## File Changes

- `extensions/dashboard/footer.ts` — raised-mode layout refactor, duplicate-footer cleanup, conditional recovery rendering, OpenSpec row formatting cleanup
- `extensions/dashboard/footer-raised.test.ts` — regression coverage for pinned metadata block, non-duplicated raised output, actionable/non-actionable recovery behavior, and OpenSpec row formatting
- `extensions/dashboard/types.ts` — any small type adjustments needed for footer composition or recovery actionability checks
- `extensions/dashboard/index.ts` — only if required for render-state wiring; avoid unrelated command-surface churn
- `extensions/web-ui/state.ts` — expose pinned dashboard metadata and recovery actionability through the backend/web UI snapshot builder
- `extensions/web-ui/types.ts` — extend dashboard/recovery snapshot types for the cleaned-up metadata contract
- `extensions/web-ui/*.test.ts` — add/update regression coverage for dashboard metadata parity in backend/web UI state
- `docs/dashboard-cleanup.md` — keep design-tree file scope and constraints synchronized with implemented reality
