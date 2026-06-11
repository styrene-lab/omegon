+++
id = "7a4d24f1-1de3-4028-aac8-5b9b31a5a2e8"
kind = "design_node"

[data]
title = "TUI Work Prep After Backend Surface Consolidation"
status = "decided"
issue_type = "planning"
priority = 1
dependencies = ["3c685f65-bc3a-4d34-8f03-11a9f2de07b9"]
open_questions = []
tags = ["tui", "backend", "acp", "console", "capabilities", "release-0.27.0"]
+++

## Overview

The 0.27 backend consolidation leaves the TUI with stable read-only backend seams to consume instead of scraping tool adapters or inline ACP JSON. This node records the immediate handoff state for focused TUI work.

## Available substrates

- Lifecycle read projections: ACP exposes `_lifecycle/snapshot`, `_lifecycle/design/list`, `_lifecycle/design/get`, `_lifecycle/design/ready`, `_lifecycle/design/blocked`, and `_lifecycle/design/frontier`.
- Backend surface registry: `core/crates/omegon/src/backend.rs` is the source of truth for ACP method discovery, planned HTTP aliases, mutability, permissions, and side-effect labels.
- Provider telemetry discovery: `_provider/retry`, `_provider/failure`, and `_turn/cancelled` are registered as read-only notification contracts.
- Capability inventory: ACP exposes `_capabilities/inventory` for installed extensions, Armory assets, and catalog agents.
- Console/backend direction docs: `docs/acp-expansion-integration-surface.md` and `docs/omegon-console-backend-surface.md` capture the ACP-vs-native-API boundary for later Dioxus/console work.

## Decisions

### Decision: Start with Ratatui as the first backend-surface consumer

Status: accepted

Rationale: The existing TUI can validate the backend registry, capability inventory, lifecycle projections, and telemetry contracts before a larger Dioxus/console UI is introduced.

### Decision: Keep mutation paths service-backed

Status: accepted

Rationale: TUI editing/control actions must call lifecycle/domain services, not design-tree tool adapter functions. Tool handlers stay transport adapters.

### Decision: Keep ACP session semantics separate from control-plane APIs

Status: accepted

Rationale: ACP remains the conversation/session protocol. Runtime, lifecycle, capability, provider, and dashboard state should flow through backend/control-plane projections that ACP and future HTTP surfaces can share.

## Immediate TUI work plan

1. Add a capability/status view backed by `_runtime/capabilities` plus `_capabilities/inventory`.
2. Add lifecycle projection views/actions backed by lifecycle read handles rather than design-tree tool handlers.
3. Display provider retry/failure/turn-cancelled telemetry through event surfaces instead of assistant-authored transcript text.
4. Keep mutation work behind lifecycle mutation services; do not route TUI editing through tool adapter functions.

## Guardrails

- Cargo only accepts one test filter per `cargo test` invocation; run separate invocations for multiple filters.
- Treat `backend.rs` as metadata/discovery, not request dispatch.
- Preserve stable `_runtime/capabilities` response shape for current ACP clients while adding richer metadata through additive fields or separate surfaces.

## Validation baseline

Recent focused validation before this handoff:

- `cargo fmt --check`
- `cargo check -p omegon`
- `cargo test -p omegon capabilities:: -- --nocapture` — 15 passed
- `cargo test -p omegon backend::tests -- --nocapture` — 5 passed
- `cargo test -p omegon acp::extension_metadata_tests::runtime_capabilities_advertise_secret_surfaces -- --nocapture` — 1 passed
