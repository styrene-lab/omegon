+++
id = "066e0116-1dde-4f25-90a0-5cd39a09ae6f"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Add board and dashboard views for design-tree task workflows

## Overview

Add workflow-oriented read views over the design tree: kanban/board grouping by status, milestone summaries, overdue indicators, and compact task-management surfaces in both the TUI dashboard and the web UI.

## Decisions

### Decision: Board/dashboard views are downstream read surfaces over the single-repo design-tree query model

**Status:** decided

**Rationale:** This child should not invent new storage or bespoke selection logic. It consumes the metadata and filtering/query surface defined by the task-fields child, then presents status-grouped and milestone-oriented read views in the TUI/web layers.

## Open Questions

- What is the minimum first implementation surface: TUI dashboard only, web UI only, or both in the same change? The node overview currently promises both, which is too broad for a first implementation slice.
- What canonical board columns should exist: the full node lifecycle (seed/exploring/resolved/decided/implementing/implemented/blocked/deferred/archived) or a collapsed workflow projection? Rendering every raw status may produce a useless board.
- Should milestone summaries and overdue indicators be computed entirely from `design_tree(list)` results, or does this child require new aggregate/query endpoints first? The dependency on task-fields/query work needs to be made explicit in the UI contract.
