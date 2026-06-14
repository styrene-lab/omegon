---
id: tui-polish
title: "TUI Polish Workstream"
status: implementing
tags: [tui, polish, ratatui, tachyonfx]
open_questions:
  - "[assumption] Remaining 0.27.0 UI polish should be tracked in this umbrella node instead of separate release workstream documents."
  - "Should startup warning grouping be completed for 0.27.0 or explicitly deferred?"
  - "Should `/auth status` polish stay in TUI polish or move to provider/auth closeout?"
  - "Should footer-local styled flex rows remain local, or should shared inline rendering grow styled-cell support?"
dependencies: []
related: [release-0-27-0-workstream-ui-polish]
---

# TUI Polish Workstream

## Overview

Explore and implement Ratatui widget, TachyonFX, and UI chrome polish for the single-line TUI surfaces recently factored into shared glyph/horizontal-line grammar. Target low-risk visual improvements to workbench, active tool stream, separators, tool surface, footer/status chrome, and peer-agent representation without recoupling surface state.

## Decisions

### Segment presentation hierarchy separates prose from structured output

**Status:** accepted

**Rationale:** Conversation rendering should distinguish role/provenance from content form. Assistant responses and tool results such as reading a markdown file can both be prose/markdown and should share the same prose rendering path, while structured outputs use structured renderers. Segment self-reporting should project both axes: who/what produced the segment and what kind of content it contains.


## 0.27.0 absorbed release workstream

This node now subsumes [[release-0.27.0-workstream-ui-polish|0.27.0 workstream — UI polish]] for the remaining 0.27.0 TUI/UI work. Treat the release workstream document as historical handoff/progress evidence; track new UI polish decisions, open questions, and remaining release actions here.

## Completed on main

| Surface / concern | Status | Evidence |
|---|---|---|
| Slim Workbench plan overflow | Done | Active/todo plan rows are prioritized over completed rows when height-constrained; focused `slim_plan` tests cover actionable rows and hidden-count truthfulness. |
| Disconnected footer remediation | Done | Engine/footer copy names the selected provider and exact `/login <provider>` remediation for disconnected route state; focused `left_panel` tests cover provider-specific copy. |
| Slim tool inline affordances | Done | `surfaces::inline` and `tui::inline_render` provide renderer-neutral inline rows, compact `⌃O details`, and right-aligned affordance layout; `slim_tool_` and `inline` tests cover alignment/truncation. |
| Footer engine row alignment | Done | `engine_flex_row` preserves label/value styling while right-aligning values and truncating long values; `engine_flex_row` and `left_panel` tests cover behavior. |
| Release branch merge-forward | Done | PR #149 merged into `release/0.27`; `release/0.27` has been merged forward into `main`, including inline row and footer work. |

## Remaining 0.27.0 UI polish

| Surface / concern | Classification | Next action |
|---|---|---|
| `/auth status` readability | Remaining | Inspect the current command output and decide whether TUI polish owns formatting tweaks or whether this belongs to provider/auth closeout. |
| Startup warning grouping | Remaining / likely defer | Verify current startup warning presentation. If low-risk grouping is obvious, implement; otherwise record a 0.27.0 deferral so it stops floating as implied scope. |
| Workbench visual hierarchy after merge-forward | Verification | Re-run focused plan/workbench tests and inspect whether the merged release work still satisfies active/todo visibility on `main`. |
| Active tool stream polish | Open umbrella area | Keep future work low-risk and semantic-surface based; do not recouple tool stream state to renderer-specific strings. |
| Separators / footer / status chrome | Open umbrella area | Use the inline/flex-row grammar for single-line rows where possible; preserve local styled helpers where shared text rendering would lose semantic styling. |
| Peer-agent representation | Open umbrella area | Preserve the accepted presentation hierarchy decision: producer/provenance is separate from content form. |

## Current decisions

### Inline/flex rows are the canonical single-line affordance grammar

**Status:** accepted

**Rationale:** Single-line rows that combine content with keyboard affordances should model left content and right affordance groups separately, with an elastic spacer and display-width-aware truncation. This avoids left-flow `Ctrl+O`/`Ctrl+S` prose drifting across tool headers, slim rows, and footer/status chrome. Styled surfaces may keep local Ratatui helpers when shared text rendering would lose label/value styling, but they should preserve the same layout contract.

## Current operating rules

- Prefer semantic projections and shared row contracts over renderer-specific string concatenation.
- Do not hide route/auth diagnostic detail; use concise visible summaries plus detailed status surfaces.
- Avoid broad layout churn for 0.27.0; polish must be small, testable, and release-safe.
- When a scoped release workstream is complete, fold its status back into this umbrella node rather than creating parallel active TUI nodes.
