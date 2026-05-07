+++
id = "f7b1cac7-7dbf-42a4-a7f9-8d497a9654b8"
kind = "document"
title = "Native plan mode — structured task decomposition with TUI widget and Auspex/browser integration"
status = "exploring"
tags = ["rust", "tui", "planning", "auspex", "openspec", "design-tree"]
aliases = ["native-plan-mode"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
dependencies = ["embedded-web-dashboard"]
issue_type = "epic"
open_questions = ["Should the plan TUI widget live in the dashboard panel, the conversation stream, or both? Dashboard gives persistent visibility; conversation keeps context inline.", "The local `/dash` compatibility surface currently serves basic HTML via the embedded axum server. What's the frontend stack for the enriched plan/lifecycle view in Auspex and, where still needed, the local browser fallback — raw HTML+JS, HTMX, or full Dioxus WASM (mdserve-dioxus-frontend is already a seed node)?"]
parent = "conversation-rendering-engine"
priority = "2"
related = ["mdserve-dioxus-frontend"]
+++

# Native plan mode — structured task decomposition with TUI widget and Auspex/browser integration

## Overview

The Rust TUI needs native task planning — structured decomposition, dependency ordering, interactive approval, and progress tracking. Two surfaces: (1) TUI widget — compact plan view in the conversation or dashboard showing tasks with status badges, dependency arrows, and approve/reject controls. (2) Browser view — Auspex should be the primary rich plan viewer, with the existing `/dash open` localhost UI kept only as a local compatibility surface until behavior migrates. The browser surface should show the full design tree, implementation specs with Given/When/Then scenarios, task progress, and plan history. This is not a separate planning system — it surfaces the same lifecycle data (design nodes, OpenSpec changes, cleave decomposition) through a visual plan interface. The TUI widget shows the current plan inline; the browser view shows the full graph. OpenCrabs' PlanDocument model is a useful reference for the data structure: typed tasks with dependencies, complexity scores, acceptance criteria, and status transitions. But our version should be backed by the existing design-tree + OpenSpec artifacts rather than a separate plan store.

## Open Questions

- Should the plan TUI widget live in the dashboard panel, the conversation stream, or both? Dashboard gives persistent visibility; conversation keeps context inline.
- The local `/dash` compatibility surface currently serves basic HTML via the embedded axum server. What's the frontend stack for the enriched plan/lifecycle view in Auspex and, where still needed, the local browser fallback — raw HTML+JS, HTMX, or full Dioxus WASM (mdserve-dioxus-frontend is already a seed node)?
