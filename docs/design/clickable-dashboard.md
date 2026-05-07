+++
id = "fa61a9e5-2f28-4f3e-904c-abcb8267c4d6"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Clickable Dashboard Items — URI Routing Integration

## Overview

Wire the URI resolver into dashboard rendering so users can click on Design Tree nodes, OpenSpec changes, and other items to open them in the appropriate viewer (mdserve for markdown, editor for code, obsidian for excalidraw). This makes the dashboard much more interactive and useful by reducing the friction between viewing project status and accessing the underlying files.

## Research

### Clickable Dashboard Items Analysis

**Design Tree Items:**
- Each `DesignNode` has a `filePath` property pointing to the markdown file
- Rendered in both footer (compact/raised) and overlay modes
- Status: Easy to wire - direct file path available

**OpenSpec Items:**
- Each `ChangeInfo` has a `path` property (directory)
- Contains multiple files: `proposal.md`, `design.md`, `tasks.md`, `specs/*.md`
- Could link to proposal.md by default, or provide multiple click targets
- Status: Moderate complexity - need to choose target file or provide multiple options

**Cleave Items:**
- `CleaveChildState` items don't directly reference files
- Could potentially link back to the parent task or OpenSpec change
- Less critical since they're primarily status indicators
- Status: Low priority - mainly runtime status

**Current UI Touch Points:**
1. Footer compact mode: terse summaries with counts
2. Footer raised mode: expanded lists with icons and progress
3. Overlay mode: interactive lists with expand/collapse

**Integration Strategy:**
- Import URI resolver into dashboard components
- Wrap item text with OSC 8 hyperlinks using `osc8Link()`
- Ensure graceful degradation in terminals without OSC 8 support
- Use getMdservePort() to detect if mdserve is running

## Decisions

### Decision: Create dashboard URI helper module

**Status:** decided
**Rationale:** Create `extensions/dashboard/uri-helper.ts` that imports the URI resolver and provides dashboard-specific functions for generating clickable links. This keeps the logic centralized and makes it easy to maintain consistent behavior across footer and overlay components.

### Decision: OpenSpec default to proposal.md

**Status:** decided
**Rationale:** For OpenSpec change items, link to `proposal.md` by default since it's the entry point that explains what the change is about. Later could add modifier keys (ctrl+click for design.md, shift+click for tasks.md) but start simple with a single default target.

### Decision: Dashboard overlay supports click and keyboard open

**Status:** decided
**Rationale:** Because OSC 8 click behavior varies by terminal and keyboard shortcuts can be unreliable globally, the dashboard overlay provides both explicit clickable links and a local 'o' action that opens the selected item target using the OS URI handler.

### Decision: Dashboard footer can subsume harness footer metrics

**Status:** decided
**Rationale:** The dashboard already owns the footer surface and can render harness-derived signals directly. We can keep the useful token flow/model/session information while dropping the redundant raw context percentage display in favor of the dashboard's color-coded context bar plus explicit context-window size.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/dashboard/uri-helper.ts` (new) — URI resolution utilities for dashboard items
- `extensions/dashboard/footer.ts` (modified) — Add clickable links to Design Tree and OpenSpec items in compact/raised modes
- `extensions/dashboard/overlay.ts` (modified) — Add clickable links to interactive overlay list items
- `extensions/design-tree/dashboard-state.ts` (modified) — Emit design node file paths into dashboard state so dashboard renderers can link to design docs
- `extensions/openspec/dashboard-state.ts` (modified) — Emit OpenSpec change directory paths into dashboard state so dashboard renderers can link to proposal.md
- `extensions/dashboard/overlay-data.ts` (modified) — Wrap overlay Design Tree and OpenSpec item labels with OSC 8 links via the dashboard URI helper
- `extensions/dashboard/uri-helper.test.ts` (new) — Tests for dashboard URI linking behavior and OpenSpec proposal fallback
- `extensions/dashboard/footer.ts` (modified) — Artifact badges in raised OpenSpec footer now link individually to proposal.md, design.md, and tasks.md when present
- `extensions/dashboard/overlay-data.ts` (modified) — Expanded OpenSpec overlay rows now render artifact-specific clickable file rows for proposal, design, and tasks
- `extensions/dashboard/overlay-data.test.ts` (modified) — Coverage for expanded OpenSpec artifact rows in overlay builders

### Constraints

- All dashboard tests must continue passing
- Graceful degradation in terminals without OSC 8 support
- Import URI resolver without coupling to view extension internals
- Use getMdservePort() from vault extension for mdserve detection
- Dashboard state now carries optional file path metadata for linkable items
- OpenSpec artifact rows only link files that exist on disk; missing artifacts remain plain text or omitted
