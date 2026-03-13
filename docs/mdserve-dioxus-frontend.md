---
id: mdserve-dioxus-frontend
title: "mdserve: Dioxus WASM frontend — lifecycle/intelligence views"
status: seed
parent: markdown-viewport
dependencies: [mdserve-lifecycle-backend]
tags: [rendering, dioxus, wasm, frontend, lifecycle]
open_questions: []
issue_type: feature
---

# mdserve: Dioxus WASM frontend — lifecycle/intelligence views

## Overview

Dioxus WASM SPA served at /dashboard, /graph, /board from the mdserve binary. Reactive components for: design tree graph (fdg force-directed layout), OpenSpec stage funnel, kanban board (node status swim lanes), memory fact graph, cleave execution timeline. Consumes /api/ routes from the lifecycle backend. Built separately with dioxus-cli, embedded in binary via include_bytes!. This is the highest-uncertainty piece — fdg WASM integration and WebSocket reactive state management in Dioxus signals are the main unknowns.

## Open Questions

*No open questions.*
