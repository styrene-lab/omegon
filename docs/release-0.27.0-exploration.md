+++
id = "release-0-27-0-exploration"
kind = "document"
title = "0.27.0 release exploration"
status = "exploring"
tags = ["release", "0.27.0", "hardening", "ui", "auth", "provider-routing"]
aliases = ["0.27.0 release exploration", "release-0.27.0"]
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# 0.27.0 release exploration

## Purpose

This document captures release exploration findings for the 0.27.0 line: feature work to target after the release, bug fixes that should be considered release blockers or release-hardening work, hardening opportunities, and UI polish opportunities.

This is a working document, not a final release note. The release note source of truth remains `CHANGELOG.md`; this document is for triage and prioritization.

## Current release posture

0.27.0 is already a large surface-coherence and provider-routing line. Recent commits and changelog entries show major work in four clusters:

1. **Provider/auth route control** — explicit startup/login/model-switch routing, fallback/disconnected route state, route warnings, `/auth status` route details, and credential diagnostics.
2. **Workbench/lifecycle visibility** — Plan Dock promoted to Workbench, slim-mode plan rows, lifecycle workstream projection, and active/incomplete plan retention.
3. **Console/ACP backend surfaces** — assistant capability inventory, assistant readiness, assistant run read models, secret readiness, blocked/completed run states.
4. **Conversation/presentation semantics** — peer-agent conversation representation, semantic tool chrome/glyph registry, producer identity separated from content form.

The line is feature-rich enough that further broad feature work should be biased toward post-release unless it closes an observable release blocker.

## Findings: bug fixes to target for this release

### 1. Auth store integrity: OpenAI/Codex credential disappearance

**Status:** active hardening in progress.

Observed failure:

- Startup selected `openai-codex:gpt-5.5`.
- `auth.json` existed at `/Users/wilson/.config/omegon/auth.json`.
- The startup auth probe reported `provider_entry_exists=false` for `openai-codex`.
- `/login openai-codex` then persisted a fresh `openai-codex` entry with `accountId` and subsequent probes resolved it successfully.

What is known:

- Provider mapping is correct: `openai-codex` uses auth key `openai-codex` and env var `CHATGPT_OAUTH_TOKEN`.
- The failure was not env-var priority/order; the stored provider entry was absent at startup.
- External Codex CLI rescue was unavailable because `~/.codex/auth.json` did not exist.

Release-target fix/hardening already started:

- Auth write paths now trace provider key-set deltas on credential writes, refreshes, and logout.
- Auth updates now refuse to replace an unparsable existing `auth.json` with a partial store.
- Regression coverage verifies writing another provider preserves existing `openai-codex` credentials and malformed auth stores are not overwritten.

Release blocker question:

- Run one relaunch cycle with the patched binary and confirm startup hydrates/resolves existing `openai-codex` without `/login`.

### 2. Startup route truthfulness must remain non-negotiable

0.27.0 changed startup fallback behavior from implicit to explicit. The release should not ship any path where the TUI footer, session log, or route state claims one provider/model while requests are served by another.

Must verify before release:

- Selected provider missing credentials + no explicit fallback enters disconnected state.
- Selected provider missing credentials + explicit fallback displays both selected and served model accurately.
- `/login` transitions route state to connected and updates visible status without restart.
- `/model` and model-tier switches route through the controller and do not bypass diagnostics.

### 3. OAuth callback accept-loop regression risk

Recent fix: OAuth callback listeners now accept-loop instead of failing on speculative preconnect/favicon/stale-tab requests.

Release validation should cover:

- Anthropic login.
- OpenAI/Codex login.
- Antigravity login if available.
- Stale browser tab hitting callback endpoint before the actual OAuth redirect.

This is release-relevant because login reliability is foundational for the provider-routing changes.

### 4. `just link` and binary freshness

Recent fix: `just link` rebuilds release binary before linking so `omegon --version` cannot point at stale HEAD.

Release validation should include:

- `just link` from a dirty-but-source-valid tree does not install stale artifacts.
- `omegon --version` after `just link` matches current commit/version metadata.
- The installed binary contains the latest auth tracing/hardening before relaunch testing.

### 5. Changelog structure has duplicate `[Unreleased]` subsection headings

Current `CHANGELOG.md` now has `### Added`, `### Fixed`, then another `### Added`/`Changed`/`Fixed` sequence under `[Unreleased]`. This is valid Markdown but weak release hygiene.

Release-target fix:

- Consolidate `[Unreleased]` headings into canonical Keep-a-Changelog order before tagging.
- Move release-hardening auth entries into the right section.

## Findings: hardening opportunities

### 1. Auth store write audit trail

The new key-delta traces make future credential disappearance attributable through normal write paths. More hardening options:

- Include a short hash/fingerprint of the provider key set, not credential values.
- Log writer operation context for startup import, external adoption, refresh, login, logout, and API-key login distinctly.
- Consider a `.bak` file for the last valid `auth.json` before each atomic write.
- Consider schema validation before accepting existing auth stores.
- Consider a repair command: `omegon auth doctor --repair`.

Tradeoff: backup/repair adds storage and recovery complexity; trace-only is lower risk for 0.27.0.

### 2. Auth store concurrency and lock robustness

Current lock path is an adjacent `.lock` file with retry. Opportunities:

- Include lock holder PID/timestamp in the lock file.
- On timeout, report stale lock diagnostics.
- Consider stale-lock recovery with age threshold.
- Add tests for concurrent writes preserving all providers.

### 3. Provider-route state machine coverage

The provider-route controller is now central. It needs invariant tests:

- Every public path that changes auth/model state emits a route event.
- Route selected/served/disconnected/fallback state is serializable to TUI, ACP, and CLI status from the same DTO.
- Route state cannot claim connected if credential ledger says missing/expired.

### 4. Release preflight for operator workflow regressions

Add or document a focused local release-hardening checklist:

- `just test-commit`
- `just lint`
- targeted auth tests
- manual login smoke for OAuth providers
- `just link`
- relaunch with `openai-codex:gpt-5.5`
- `/auth status`
- `/model` switch smoke
- Workbench active-plan smoke

### 5. Extension SDK compatibility warnings

Recent startup logs show incompatible or missing local extensions:

- `omegon-design` SDK contract older than minimum.
- `omegon-voice` SDK contract older than minimum.
- `aether`/`shuttle` missing binaries.
- `vox` permission denied.

These are not necessarily core release blockers, but 0.27.0 surfaces extension capability inventory more prominently. The operator experience should distinguish core release health from local extension drift.

Opportunity:

- Group extension warnings into a concise startup/diagnostic surface.
- Provide `omegon extension doctor` guidance or reuse capability inventory readiness fields.

## Findings: future feature work

### 1. Auth doctor and credential provenance UI

Build on provider-route diagnostics with a first-class `auth doctor` flow:

- Show auth path source (`default` vs `OMEGON_AUTH_JSON_PATH`).
- Show provider entries present/missing/expired without secrets.
- Show external fallback availability (`~/.codex/auth.json`, Claude Code, Gemini CLI, etc.).
- Show last auth-store mutation operation from logs/audit trail.
- Offer safe repair: restore backup, re-import external credential, or re-login.

### 2. Unified route/state projection across TUI, CLI, ACP

The release line introduced route-control pieces. Future work should complete the semantic projection:

- One provider route DTO consumed by TUI footer, `/auth status`, ACP, and session logs.
- Include selected model, served model, credential source, fallback reason, and remediation.
- Avoid renderer-specific route logic.

### 3. Workbench as operational state, not only UI

Workbench is now the visible operational state for plans, cleave, delegate, and workstreams. Future work:

- Persist Workbench state explicitly enough to recover after restart.
- Add Workbench reconciliation warnings when assistant final claims conflict with active tasks.
- Connect Workbench rows to lifecycle/design/OpenSpec provenance.
- Provide command registry actions for Workbench operations instead of TUI-only arms.

### 4. Capability inventory as release/readiness substrate

The console/ACP capability inventory work can become a release readiness layer:

- Installed extensions and SDK compatibility.
- Secret readiness.
- Assistant launch readiness.
- Package/helper tool availability.
- Provider/auth route readiness.

Future feature: `omegon readiness` or `omegon doctor` that assembles these into a single operator-facing health report.

### 5. Prompt/loop provenance safety

Recent prompt-surface directives call out prompt templates and `/loop` as executable instruction sources. Future work:

- Preview/validate prompt templates before execution.
- Record provenance for queued prompts.
- Add explicit safety handling for repeated `/loop` execution.
- Share command registry safety metadata with ACP/CLI/TUI.

## Findings: UI polish opportunities

### 1. Provider route footer clarity

Shipped release-polish update: disconnected Slim engine footer rows now name the selected provider and exact remediation command, e.g. `OpenAI/Codex login required` plus `/login openai-codex`, instead of a generic provider warning. Focused `left_panel` tests cover the copy and stale-row cleanup.

The footer/model card should clearly distinguish:

- selected profile model
- served bridge model
- fallback provider
- disconnected/login-required state
- credential source and expiry state where useful

Avoid dense text in Slim mode. Suggested pattern:

- Connected: `Codex · gpt-5.5 · OAuth`
- Fallback: `Selected Codex unavailable → serving Claude Fable`
- Disconnected: `Codex login required · /login openai-codex`

### 2. Workbench visual hierarchy

Shipped release-polish update: Slim Workbench plan overflow now prioritizes actionable rows (`active`, then `todo`) before completed rows, preserving full plan context while hiding done items first when vertical space is constrained. Focused `slim_plan` tests cover the policy and hint behavior.

Workbench has accumulated plan rows, workstreams, delegate/cleave progress, and lifecycle state. Remaining polish opportunities:

- Use stable glyph grammar for todo/in-progress/done/blocked.
- Keep Slim mode compact but preserve actionable state.
- Avoid duplicate tool-call cards when Workbench already represents the operation.
- Add stale/inconsistent indicators when plan state and assistant prose diverge.

### 3. Startup warning grouping

Startup currently can emit many warnings: provider auth, extension SDK drift, missing binaries, route disconnected state, web auth mode, etc.

Polish:

- Group warnings by domain: Auth, Extensions, Tools, Project.
- Show high-priority remediation first.
- Keep noisy extension drift out of the main route/auth warning unless it blocks the selected task.

### 4. `/auth status` readability

`/auth status` should be the operator’s first stop for login issues.

Polish opportunities:

- Show selected route and served bridge at top.
- Show auth store path and source.
- Show provider table with: status, credential source, expiry/refreshable, external fallback available.
- For missing selected provider, print exact remediation command.

### 5. Release/doctor UI

A release-hardening line benefits from an operator-facing health command:

- `omegon doctor`
- `omegon auth doctor`
- `omegon extension doctor`
- `omegon release preflight`

This can start as text output and later project into TUI/ACP.

## Release mechanics findings

### Version/tag state

- Workspace version is currently `0.27.0` in `Cargo.toml`.
- `core/release.toml` is configured for shared workspace versioning and `v{{version}}` tags.
- The `just release` recipe expects a clean tree, runs `just preflight`, verifies a stable semver version, updates milestone state, commits `chore(release): <version>`, tags `v<version>`, and builds the release binary.
- `just preflight` is the intended release-readiness guard; it checks branch/version/changelog/install docs/manifest wiring before mutation.
- Release branch helpers exist: `just branch-release` and `just merge-release-forward`, matching the documented trunk + `release/X.Y` stabilization model.

Implication: if this is still pre-tag 0.27.0 stabilization, release-hardening commits should either land on `release/0.27` or land on `main` before `just release`. If `v0.27.0` has already been published, these findings should be reframed as 0.27.1 patch hardening rather than a refreshed 0.27.0 artifact.

### Available local validation gates

Focused gates observed in `justfile`:

- `just test-commit` — changed-crate Rust validation for focused commits.
- `just test-rust` — full Rust workspace test gate.
- `just lint` — fmt, check, clippy for full workspace/all targets.
- `just upstream-provider-check` — cheap provider drift and upstream version checks.
- `just preflight` — release preflight before release mutation.
- `just link` — rebuild/install dev binary.

Release-hardening validation should use focused tests during iteration, then at minimum `just test-commit`, `just lint`, `just preflight`, and `just link` before tagging. Full `just test-rust` remains the stronger release gate when time permits.

### Provider-route code surface requiring release attention

The active route implementation lives in `core/crates/omegon/src/route.rs` with:

- `ProviderRoute::{Serving, Fallback, LoginPending, Disconnected}`.
- `CredentialLedger` as the real credential probe.
- `RouteController::resolve_startup` for selected/fallback/disconnected startup state.
- route events consumed by IPC/MQTT/TUI surfaces.

Risk area: this is now central infrastructure. Any remaining path that creates an LLM bridge or changes provider/model/auth state outside `RouteController` is a release risk because it can make status surfaces lie.

## Parallelizable workstreams

The three most obvious parallelizable workstreams are:

1. **Release mechanics and validation hygiene** — owned by this Omegon/operator session on `release/0.27-mechanics-validation`. Scope: changelog normalization, release/tag decisioning, validation gates, `just link` freshness, and final release readiness.
2. **UI polish** — owned by `omegon-secundus` on `release/0.27-ui-polish`. Scope: provider-route footer clarity, `/auth status` readability, Workbench visual hierarchy, and startup warning grouping.
3. **Auth/store integrity and provider-route correctness** — owned by `omegon-quartus` on `release/0.27-auth-integrity`. Scope: Codex credential disappearance, auth-store write safety, relaunch/no-login validation, and route-controller invariants.

Detailed handoff docs:

- [[release-0.27.0-workstream-mechanics-validation|0.27.0 workstream — release mechanics and validation hygiene]]
- [[release-0.27.0-workstream-ui-polish|0.27.0 workstream — UI polish]]
- [[release-0.27.0-workstream-auth-integrity|0.27.0 workstream — auth/store integrity and provider-route correctness]]

## Proposed release triage

### Must fix before 0.27.0 tag

- Complete and validate auth-store integrity hardening.
- Confirm relaunch with existing `openai-codex` auth does not require `/login`.
- Consolidate `[Unreleased]` changelog headings.
- Run focused provider-route/auth regression tests.
- Run `just link` and verify installed binary freshness.

### Should fix if low risk

- Add concurrent auth write preservation test.
- Add clearer auth-store parse failure operator message.
- Add route-state smoke command or checklist entry.
- Reduce startup warning noise around unrelated extension drift.

### Defer to post-release

- Full `auth doctor` repair workflow.
- Auth store backups and restore command.
- Unified readiness/doctor surface across providers/extensions/secrets.
- Workbench persistence/recovery beyond current projection.
- Larger UI layout redesigns.

## Open questions

1. Should 0.27.0 be treated as already tagged/released and this work become 0.27.1 hardening, or is this a release-line stabilization pass before a refreshed 0.27.0 artifact?
2. Do we want a `release/0.27` branch for this hardening, or continue focused direct commits on `main`?
3. Should auth-store backup/restore be included now, or is trace + parse-refusal sufficient for this release?
4. Which OAuth providers are available for manual smoke on this machine before tag?

## Immediate next steps

1. Finish the current auth-store integrity patch: run focused tests, `just lint` or `just test-commit`, then commit.
2. Relaunch the linked binary with `openai-codex:gpt-5.5` and confirm no `/login` is required.
3. Consolidate `CHANGELOG.md` `[Unreleased]` sections.
4. Decide whether remaining items are release blockers or post-release tasks.
