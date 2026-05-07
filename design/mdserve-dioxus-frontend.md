+++
id = "011ef9e9-a339-4f89-b349-72fd949af27d"
kind = "design_node"
title = "mdserve: Dioxus WASM frontend — lifecycle/intelligence views"
status = "seed"
tags = ["rendering", "dioxus", "wasm", "frontend", "lifecycle"]
aliases = ["mdserve-dioxus-frontend"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
dependencies = ["mdserve-lifecycle-backend"]
issue_type = "feature"
open_questions = []
parent = "markdown-viewport"
+++

# mdserve: Dioxus WASM frontend — lifecycle/intelligence views

## Overview

Dioxus WASM SPA served at /dashboard, /graph, /board from the mdserve binary. Reactive components for: design tree graph (fdg force-directed layout), OpenSpec stage funnel, kanban board (node status swim lanes), memory fact graph, cleave execution timeline. Consumes /api/ routes from the lifecycle backend. Built separately with dioxus-cli, embedded in binary via include_bytes!. This is the highest-uncertainty piece — fdg WASM integration and WebSocket reactive state management in Dioxus signals are the main unknowns.

## Open Questions

*No open questions.*
