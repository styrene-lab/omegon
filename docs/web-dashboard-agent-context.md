+++
id = "3e1d60a5-1dae-4e69-8c3d-09a2806fdacf"
kind = "document"
title = "Web dashboard agent context — pass active tab and visible state to the agent with prompts"
status = "archived"
tags = ["web", "dashboard", "ux", "agent", "context"]
aliases = ["web-dashboard-agent-context"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
archive_reason = "superseded"
archived_at = "1775247150"
dependencies = []
open_questions = []
parent = "auto-doc-generation"
related = []
superseded_by = "auspex-agent-context-bridge"
+++

# Web dashboard agent context — pass active tab and visible state to the agent with prompts

## Overview

When the operator sends a message from the web dashboard, the agent receives only the text — no context about what the dashboard is showing. If the user is on the Graph tab looking at the design tree visualization and asks 'what can you tell me about this graph?', the agent has no idea what graph they mean.

The fix: when the web UI sends a user_prompt, include metadata about the active view — which tab is selected, what data is visible (e.g. the list of graph node IDs on screen), and any selection state. The agent can then use this context to provide relevant answers without the user having to explain what they're looking at.
