+++
id = "3c685f65-bc3a-4d34-8f03-11a9f2de07b9"
kind = "design_node"

[data]
title = "ACP Backend Surface Registry"
status = "implemented"
issue_type = "feature"
priority = 1
dependencies = []
open_questions = []
tags = ["acp", "backend", "lifecycle", "surfaces", "registry", "release-0.27.0"]
+++

## Overview

The ACP lifecycle read surfaces exposed lifecycle projections to headless clients, but `_runtime/capabilities` still carried a hardcoded inline map of ACP extension methods. That made the next ACP expansion fragile: every new ACP, HTTP, or console backend surface could drift across transport adapters.

This node tracks the 0.27.0 foundation work to introduce a metadata-only backend endpoint registry shared by protocol adapters.

## Outcome

Implemented in commit `937db59 feat(acp): add backend surface registry`, then extended with issue #128 telemetry discovery for `_provider/retry`, `_provider/failure`, and `_turn/cancelled`.

### Files

- `core/crates/omegon/src/backend.rs`
- `core/crates/omegon/src/acp.rs`
- `core/crates/omegon/src/main.rs`
- `CHANGELOG.md`

## Decisions

### Decision: Use a metadata-only registry first

Status: accepted

Rationale: Dispatch still belongs in the ACP, HTTP, slash, and tool adapters that own transport semantics. A registry-only slice gives clients a single capability inventory without coupling unrelated dispatch paths too early.

### Decision: Keep ACP capability output stable

Status: accepted

Rationale: `_runtime/capabilities` now derives `surfaces` from the registry, but preserves the existing `{ "version": 1 }` shape per ACP method so existing Flynt/Zed clients do not need to change.

### Decision: Record planned HTTP aliases for lifecycle surfaces

Status: accepted

Rationale: Lifecycle read projections are the first ACP surfaces likely to be reused by HTTP/console clients. Capturing the planned HTTP paths now prevents route-name drift when the HTTP adapter is wired later.

## Validation

- `cargo test -p omegon acp::tests -- --nocapture` — 30 passed
- `cargo test -p omegon acp::extension_metadata_tests -- --nocapture` — 26 passed
- `cargo test -p omegon backend::tests -- --nocapture` — 4 passed
- `cargo check -p omegon` — passed
- `cargo fmt --check` — passed
- `git diff --check` — passed

## Next expansion targets

- Add richer `_runtime/capabilities` metadata for clients that want domains, permissions, mutability, transports, and side-effect labels.
- Wire HTTP lifecycle read routes from the same registry once the HTTP/control adapter is ready.
- Add registry coverage tests that compare implemented ACP request handlers with registered ACP extension methods.
- Promote additional issue #128 turn-control contracts into the registry if the ACP protocol grows beyond `_provider/retry`, `_provider/failure`, and `_turn/cancelled`.

## Open Questions

None for the implemented registry foundation.
