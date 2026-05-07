+++
id = "8d2870fa-f34c-457d-9ac5-c1d7c37685a8"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# TUI surface pass — expose new subsystems in dashboard, footer, selectors, and commands

## Intent

The Rust core has grown significantly — persona system, MCP transport (5 modes including HTTP), encrypted secrets, auth surface, harness settings, plugin CLI, context class routing, inference backends — but the TUI only partially exposes this. The footer shows persona/tone badges and MCP counts, but the dashboard shows none of it. No selector overlays for persona/tone/context-class. Several slash commands exist but have no visual feedback beyond text dumps.\n\nThis is a single coordinated pass to bring the TUI in line with the backend.
