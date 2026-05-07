+++
id = "e41f98ea-811b-499e-b360-1de482b1f675"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# mdserve: Dioxus WASM frontend — lifecycle/intelligence views

## Overview

Dioxus WASM SPA served at /dashboard, /graph, /board from the mdserve binary. Reactive components for: design tree graph (fdg force-directed layout), OpenSpec stage funnel, kanban board (node status swim lanes), memory fact graph, cleave execution timeline. Consumes /api/ routes from the lifecycle backend. Built separately with dioxus-cli, embedded in binary via include_bytes!. This is the highest-uncertainty piece — fdg WASM integration and WebSocket reactive state management in Dioxus signals are the main unknowns.

## Decisions

### Decision: Frontend work is downstream of the lifecycle backend contract, not an independent exploration track

**Status:** decided

**Rationale:** The browser UI can only be meaningfully evaluated once the backend has decided what entities, snapshot shapes, and push/update semantics exist. This child should consume that contract rather than invent its own assumptions in parallel.

## Open Questions

- What is the minimum first frontend surface: one graph/dashboard route proving the reactive stack, or the full /dashboard + /graph + /board route set described in the overview? The current node is too broad for a first implementation pass.
- Is Dioxus still the right frontend technology once the backend contract is concrete, or would the lower-complexity embedded-web-dashboard stack (small JS bundle with force-graph/SVG) be sufficient for Auspex too? This node assumes Dioxus without revalidating that cost.
- What layout/rendering responsibilities should live client-side versus server-side for the graph and board views? fdg-in-WASM is one option, but server-computed layouts may simplify portability and packaging.
