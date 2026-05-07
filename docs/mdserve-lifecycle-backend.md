+++
id = "c7ba42de-c4c1-4a7a-9e60-75610d382a41"
kind = "document"
title = "mdserve: Lifecycle data model + /api/ backend"
status = "exploring"
tags = []
aliases = ["mdserve-lifecycle-backend"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
dependencies = []
issue_type = "feature"
open_questions = ["What is the right decomposition into implementation child nodes?", "What is the canonical backend scope for v1: only design-tree/OpenSpec data, or also memory graph and cleave history? The parent overview promises all three layers, but the first backend slice should be narrower.", "Should the lifecycle backend read directly from repo artifacts on each request, maintain an in-memory indexed snapshot with file-watch invalidation, or persist a derived cache? The serving model affects both API shape and daemon complexity.", "What API contract is stable enough for the frontend: entity-specific routes (/api/design-tree, /api/openspec, /api/cleave-history) only, or also a normalized portal snapshot endpoint for coarse-grained refresh?", "How should websocket push be modeled for lifecycle updates: push full snapshots after file changes, typed invalidation events, or per-entity incremental updates? Another agent cannot implement backend and frontend independently without this contract."]
parent = "markdown-viewport"
priority = "1"
related = []
+++

# mdserve: Lifecycle data model + /api/ backend

## Overview

> Parent: [Omegon Rendering Engine — Lifecycle Visualization & Project Intelligence Layer](markdown-viewport.md)
> Spawned from: "What is the right decomposition into implementation child nodes?"

*To be explored.*

## Decisions

### Decision: The lifecycle backend is the first active Auspex implementation slice

**Status:** decided

**Rationale:** Everything else in the browser portal depends on a coherent domain/API layer. The Dioxus frontend and the Nix packaging story both become easier once the backend contract and repo-scanning model are concrete. This node should be explored and implemented before frontend polish or packaging finalization.

## Open Questions

- What is the right decomposition into implementation child nodes?
- What is the canonical backend scope for v1: only design-tree/OpenSpec data, or also memory graph and cleave history? The parent overview promises all three layers, but the first backend slice should be narrower.
- Should the lifecycle backend read directly from repo artifacts on each request, maintain an in-memory indexed snapshot with file-watch invalidation, or persist a derived cache? The serving model affects both API shape and daemon complexity.
- What API contract is stable enough for the frontend: entity-specific routes (/api/design-tree, /api/openspec, /api/cleave-history) only, or also a normalized portal snapshot endpoint for coarse-grained refresh?
- How should websocket push be modeled for lifecycle updates: push full snapshots after file changes, typed invalidation events, or per-entity incremental updates? Another agent cannot implement backend and frontend independently without this contract.
