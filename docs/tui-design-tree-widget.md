+++
id = "b214d3b3-a063-47e8-93b3-796dcc47a70e"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Dashboard design tree — actual tree rendering with expand/collapse

## Overview

The dashboard sidebar currently renders design nodes as a flat list with status icons. The design tree IS a tree — parent/child relationships, branching, depth. Render it with proper indentation, expand/collapse (▸/▾), and tree lines (├── └──). Same interaction model: mouse scroll, hotkey focus, arrow key navigation, Enter to expand/collapse or set focus.

The tui-tree-widget crate is already a dependency. The current implementation uses it for the node list but doesn't leverage the tree structure from the design documents. Need to build the tree from parent-child relationships in the design node data.

## Open Questions

*No open questions.*
