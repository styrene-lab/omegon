---
id: horizontal-line-system
title: "TUI Horizontal Line Grammar"
status: seed
tags: []
open_questions:
  - "[assumption] A shared horizontal-line grammar can live as a pure rendering helper without owning surface-specific data models or lifecycle state."
  - "Which first consumers should be migrated in MVP: workbench rule lines, separator component, active tool stream header, slim tool collapsed line, engine fallback/status line, or full tool card headers?"
  - "How should the shared grammar expose theme/color semantics without importing or depending on individual surface components?"
  - "What stable test contract should protect visual consistency: exact buffer snapshots, semantic span inspection, or focused width/truncation assertions?"
dependencies: []
related: []
---

# TUI Horizontal Line Grammar

## Overview

Design a shared, semantic rendering grammar for horizontal TUI lines (workbench, engine/status rows, slim/full tool call headers, separators) so width budgeting, metric ordering, color roles, and rule glyphs are consistent without re-coupling decoupled UI surfaces.

## Decisions

### Horizontal line system is a pure visual grammar module

**Status:** proposed

**Rationale:** To avoid re-coupling decoupled UI elements, the shared module must not own workbench, tool-card, footer, or segment state. It should accept a small semantic line spec plus theme/background and return Ratatui spans/lines. Surface modules remain responsible for their own data projection and choose which line specs to render.

### Tool surface glyphs use a shared semantic glyph matrix

**Status:** proposed

**Rationale:** Tool surfaces need consistent iconography for tool lifecycle, result class, detail affordances, and line/rule chrome without hardcoding glyph strings in every renderer. A shared glyph matrix keeps visuals replaceable while preserving decoupling: surfaces ask for semantic glyph roles, not specific symbols.

## Open Questions

- [assumption] A shared horizontal-line grammar can live as a pure rendering helper without owning surface-specific data models or lifecycle state.
- Which first consumers should be migrated in MVP: workbench rule lines, separator component, active tool stream header, slim tool collapsed line, engine fallback/status line, or full tool card headers?
- How should the shared grammar expose theme/color semantics without importing or depending on individual surface components?
- What stable test contract should protect visual consistency: exact buffer snapshots, semantic span inspection, or focused width/truncation assertions?
