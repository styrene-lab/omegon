+++
id = "7ab3e330-1167-4f26-bc7c-8b4d7c941ad7"
kind = "document"
title = "Wire existing Rust tool implementations as registered tools"
status = "implemented"
tags = []
aliases = ["rust-wire-existing-tools"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
parent = "ts-to-rust-migration"
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
