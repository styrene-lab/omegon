+++
id = "92349aa1-ce06-4964-894b-7c74460ea265"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Dashboard sidebar overhaul — rich tree widget with lifecycle state

## Overview

Full overhaul of the right-side dashboard panel. Replace hand-rolled tree rendering with tui-tree-widget for proper expand/collapse navigation. Rich information density at a glance — status icons, lifecycle badges, open question counts, OpenSpec progress inline. Interactive: Tab to enter sidebar, arrows to navigate, Enter to focus a node in agent context. OSC8 hyperlinks for opening OpenSpec files externally. Web dashboard deferred for deep browsing.

## Decisions

### Decision: Use tui-tree-widget for the design node tree, with rich Text per item

**Status:** decided
**Rationale:** The hand-rolled tree rendering in dashboard.rs is 30 lines of recursive code that doesn't support expand/collapse, selection, scrolling, or keyboard navigation. tui-tree-widget 0.24 is already a dependency. TreeItem accepts rich Text (multi-span Lines with colors), TreeState handles selection/open/close/scroll. Each tree item renders as: status_icon + node_id + inline badges (? count, ✓ decisions, priority). No reason to reimplement what the crate already does well.

### Decision: Sections: Header → Tree → OpenSpec → Cleave → Session — vertically stacked, tree gets remaining space

**Status:** decided
**Rationale:** The dashboard has 5 logical sections. The tree is the primary value — it should get all remaining vertical space after fixed-height sections. Header (title + pipeline bar) is 3 lines. OpenSpec changes are 1 line each + divider. Cleave only when active. Session stats at bottom (2 lines). Tree fills the gap. This ensures information density scales with terminal height.

### Decision: Focused node rendered as header section with enriched detail, not a tree selection

**Status:** decided
**Rationale:** The focused design node is the operator's current working context — it deserves a prominent header position with readiness gauge, question/decision counts, and the bound OpenSpec change name. This is separate from tree selection (which is just navigation cursor). When operator presses Enter on a tree item, it becomes the focused node (sent to agent context via design-focus bus command). The focused node in the tree gets a distinct highlight to show the connection.

### Decision: Default tree view shows non-implemented nodes only, grouped by status tier

**Status:** decided
**Rationale:** 183 of 236 nodes are implemented — showing all of them buries the actionable work. Default view filters to active nodes (implementing, decided, exploring, blocked, seed, deferred). Within the tree, parent-child structure is preserved. Implemented nodes only appear as parents when they have non-implemented children, rendered dimly. This keeps the tree compact and action-oriented.

## Open Questions

*No open questions.*
