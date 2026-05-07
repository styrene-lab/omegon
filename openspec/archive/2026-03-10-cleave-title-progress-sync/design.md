+++
id = "57fbdd3d-d0fb-42f0-99ff-ccd0f702b7cc"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# cleave-title-progress-sync — Design

## Spec-Derived Architecture

### dashboard/terminal-title

- **Terminal title reflects live cleave child progress** (added) — 1 scenarios

## Scope

<!-- Define what is in scope and out of scope -->

## File Changes

- `extensions/cleave/dispatcher.ts` — emit `dashboard:update` when child status/elapsed changes during dispatch so terminal-title and dashboard consumers refresh immediately
- `extensions/cleave/dispatcher.test.ts` — verify per-child progress updates mutate shared state and emit dashboard events
