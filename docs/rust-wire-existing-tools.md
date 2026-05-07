+++
id = "7ab3e330-1167-4f26-bc7c-8b4d7c941ad7"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Wire existing Rust tool implementations as registered tools

## Overview

view.rs, web_search.rs, local_inference.rs, render.rs already exist but aren't registered in tools/mod.rs. Wire them up with proper ToolDefinition schemas.

## Decisions

### Decision: Already wired — only small tools remain

**Status:** decided
**Rationale:** view, web_search, local_inference, render are already registered in setup.rs. The remaining unwired tools (whoami, manage_tools, switch_to_offline, 3 memory tools) are small additions.

## Open Questions

*No open questions.*
