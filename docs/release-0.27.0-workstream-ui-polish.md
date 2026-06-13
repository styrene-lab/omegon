+++
id = "release-0-27-0-workstream-ui-polish"
kind = "document"
title = "0.27.0 workstream — UI polish"
status = "exploring"
tags = ["release", "0.27.0", "workstream", "ui", "tui", "polish"]
aliases = ["0.27 UI polish", "release UI polish workstream"]
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# 0.27.0 workstream — UI polish

## Owner

Primary owner: **omegon-secundus**.

## Branch

`release/0.27-ui-polish`

Branch created from current `main` HEAD: `07027739 fix(auth): prefer refreshable oauth credentials`.

## Mission

Make 0.27.0’s operator-facing surfaces truthful, legible, and calm. The release has added provider-route state, Workbench visibility, command safety metadata, and capability/readiness surfaces; UI polish should reduce confusion without reopening core architecture.

## Inputs

- [[release-0.27.0-exploration|0.27.0 release exploration]]
- `core/crates/omegon/src/tui/footer.rs`
- `core/crates/omegon/src/tui/mod.rs`
- `core/crates/omegon/src/features/auth.rs`
- `core/crates/omegon/src/route.rs`
- Workbench/plan rendering helpers under `core/crates/omegon/src/tools.rs` and TUI modules
- startup warning paths in `core/crates/omegon/src/main.rs` / `setup.rs`

## Progress

- Shipped Slim Workbench overflow prioritization so active/todo plan rows stay visible before completed rows when the plan lane is height-constrained. Focused `cargo test -p omegon slim_plan -- --nocapture` coverage passes.
- Shipped disconnected footer remediation copy so the engine panel names the selected provider and exact `/login <provider>` command. Focused `cargo test -p omegon left_panel -- --nocapture` coverage passes.

## Current findings

- Provider-route surfaces now distinguish selected vs served route state, fallback, login-pending, and disconnected states.
- Recent bugs made truthful footer/model display release-critical; UI must not imply the selected provider is serving when fallback or disconnected state is active.
- Workbench is now operational state for plans, cleave, delegate, and lifecycle workstreams; it must stay compact in Slim mode but preserve actionable state.
- Startup logs can be noisy because provider/auth warnings and local extension drift appear in the same launch window.

## Scope

### In scope

- Improve provider route footer/model-card clarity.
- Improve `/auth status` readability around selected route, served bridge, credential state, and remediation.
- Group startup warnings by domain where low risk:
  - Auth
  - Extensions
  - Tools
  - Project
- Keep unrelated extension drift from masking selected-provider auth failures.
- Tighten Workbench visual hierarchy:
  - stable glyph grammar
  - compact plan rows
  - clear blocked/waiting/in-progress/done state
  - no duplicate successful `/plan` noise if Workbench already represents it
- Add or adjust focused snapshot/unit tests for UI formatting where practical.

### Out of scope

- Core provider-route state-machine changes.
- Auth-store write-path fixes.
- Release script mechanics.
- Large TUI redesigns or new alternate frontends.
- Extension SDK upgrades unless needed to reduce warning presentation noise.

## Acceptance criteria

- Footer/model-card copy clearly distinguishes connected, fallback, and disconnected states.
- Missing selected-provider credentials show exact remediation, e.g. `/login openai-codex`.
- `/auth status` remains readable in narrow terminals and includes selected/served route truth.
- Startup warnings are less noisy or at least more domain-separated.
- Workbench rows remain compact and actionable in Slim mode.
- Any UI changes have focused tests or snapshot-style assertions where the project already has coverage seams.

## Suggested task breakdown

1. Inspect current route/footer rendering:
   - route warning projection
   - footer model card
   - `/auth status` output
2. Draft copy variants for three states:
   - Connected: `Codex · gpt-5.5 · OAuth`
   - Fallback: `Selected Codex unavailable → serving Claude Fable`
   - Disconnected: `Codex login required · /login openai-codex`
3. Patch the smallest rendering layer that consumes existing route state.
4. Group or prioritize startup warnings without changing underlying diagnostics.
5. Run focused UI/auth/status tests.
6. Update [[release-0.27.0-exploration]] with shipped polish and remaining deferrals.
7. Commit on `release/0.27-ui-polish`.

## Risks

- UI polish can accidentally hide diagnostic detail needed for release debugging. Prefer progressive disclosure: concise visible summary plus detailed `/auth status` or log trail.
- Do not duplicate route logic in TUI. Consume semantic route/controller state.
- Avoid broad layout churn in the release branch; release polish should be minimal and testable.

## Coordination notes

- Coordinate with `release/0.27-auth-integrity` for exact auth remediation wording and credential-state fields.
- Coordinate with `release/0.27-mechanics-validation` for final changelog entries and release validation smoke.
