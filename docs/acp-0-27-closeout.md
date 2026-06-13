---
id: acp-0-27-closeout
title: "ACP 0.27.0 closeout — consolidate active ACP follow-ups"
status: implementing
tags: [acp, release, 0.27.0, closeout, flynt, readiness]
open_questions:
  - "[assumption] 0.27.0 ACP closeout should prioritize release readiness and truthfulness over new protocol feature expansion."
  - "Which active ACP follow-up items are already implemented in code but still need lifecycle/status closure?"
  - "Which remaining ACP behaviors are actual 0.27.0 blockers versus post-release follow-ups?"
dependencies:
  - provider-route-state-machine
related:
  - acp-128-turn-control-telemetry
  - acp-task-durability-contract
  - acp-session-config-surfaces
  - acp-health-permissions-diagnostics-surfaces
  - acp-extension-control-plane-hardening
  - acp-tool-package-provenance-surfaces
---

# ACP 0.27.0 closeout — consolidate active ACP follow-ups

## Purpose

Consolidate the scattered active `acp-*` design nodes into one release-focused workstream for 0.27.0 readiness. The goal is not broad ACP feature expansion; it is to determine whether the ACP surfaces already shipped for 0.27.0 are truthful, test-covered, and safe for Flynt/operator clients.

## Consolidated source nodes

This workstream absorbs the active ACP follow-ups that were inflating the active workstream count:

- `acp-128-event-plumbing`
- `acp-128-provider-events`
- `acp-128-session-cancel`
- `acp-128-turn-control-telemetry`
- `acp-ecosystem-capability-negotiation`
- `acp-extension-control-plane-hardening`
- `acp-health-permissions-diagnostics-surfaces`
- `acp-plan-task-revision-events`
- `acp-session-config-surfaces`
- `acp-task-binding-store`
- `acp-task-durability-contract`
- `acp-task-mutation-contract`
- `acp-task-stable-identity`
- `acp-task-status-error-pagination-contract`
- `acp-tool-package-provenance-surfaces`

The source nodes remain as design/reference records, but this closeout node is the single active release workstream.

## Closeout areas

### 1. ACP issue 128: turn control and provider telemetry

Verify and close the issue-128 slice:

- provider retry/failure/cancel telemetry is structured rather than assistant-authored content;
- ACP cancellation uses scoped session cancellation without killing transport;
- terminal cancellation events are visible to ACP clients;
- string-based provider telemetry detection is either removed or documented as compatibility glue;
- focused tests cover retry/failure/cancel behavior.

Primary files to inspect:

- `core/crates/omegon/src/acp_worker.rs`
- `core/crates/omegon/src/acp.rs`
- provider retry/error formatting paths in `core/crates/omegon/src/main.rs`

### 2. ACP task/direct mapping contract

Verify which task-contract features are already present and which must remain post-release:

- `stable_id`, `stable_id_quality`, `revision`, `source`, and `supported_mutations` in task projections;
- `_tasks/bind` durability semantics: `repo | session | none`;
- structured errors for stale/not-found/not-writable/conflict;
- polling-safe revision responses;
- pagination/filtering scope;
- capability advertisement that prevents clients from over-trusting partial direct mapping.

Primary files to inspect:

- `core/crates/omegon/src/plan.rs`
- `core/crates/omegon/src/tools/mod.rs`
- `core/crates/omegon/src/acp.rs`
- `core/crates/omegon/src/acp_plan_tasks.rs`

### 3. ACP session/config, diagnostics, and provenance follow-ups

Classify the issue-132 follow-ups as release blockers, implemented surfaces, or post-release work:

- `_session/status` / `_session/config` readiness;
- health/permissions/diagnostics surfaces;
- tool/package provenance surfaces versus capability inventory;
- extension control-plane hardening beyond minimal loaded/enabled invocation.

Primary files to inspect:

- `core/crates/omegon/src/backend.rs`
- `core/crates/omegon/src/capabilities/`
- `core/crates/omegon/src/acp.rs`
- `docs/acp-surface.md`
- `docs/flynt-integration.md`

## Acceptance criteria

- Active ACP lifecycle state is represented by this one closeout workstream, not fifteen parallel exploratory nodes.
- Each absorbed ACP node is classified as implemented, deferred/post-release, or a concrete subtask under this closeout.
- 0.27.0 ACP release blockers are explicitly named; non-blocking follow-ups are deferred.
- Focused ACP tests or existing test evidence are recorded for every release-blocking behavior.
- Flynt/operator-facing ACP capabilities do not advertise stronger durability, mutation, or diagnostics support than Omegon actually provides.
- Final closeout notes are reflected in `CHANGELOG.md` if behavior or operator workflow changes.

## Initial triage decision

For 0.27.0, prefer conservative capability truthfulness over direct-mapping ambition. Read-only/manual-link client behavior is acceptable if durability, mutation, and revision contracts are not fully proven. Do not block 0.27.0 on broad P1/P2 ACP diagnostics unless a shipped capability is misleading or broken.

## Triage notes — 2026-06-13

### ACP issue 128: turn control and provider telemetry

Classification: **implemented for 0.27.0; keep focused verification, not a release blocker unless tests fail**.

Evidence:

- `core/crates/omegon/src/acp_worker.rs` maps provider retry/failure/cancel agent events into typed `WorkerEvent::ProviderRetry`, `WorkerEvent::ProviderFailure`, and `WorkerEvent::TurnCancelled` variants.
- `core/crates/omegon/src/acp.rs` emits `_provider/retry`, `_provider/failure`, and `_turn/cancelled` ACP extension notifications with structured JSON payloads rather than assistant-authored text.
- ACP cancellation calls the worker cancel token and emits `_turn/cancelled` with `reason=operator_cancelled`; the transport is not killed as part of cancellation.

Remaining risk:

- Provider telemetry still carries human-readable `message` fields. That is acceptable compatibility detail if clients key on typed notification method + structured fields, not message text.

### ACP task/direct mapping contract

Classification: **partially implemented and truthfully conservative; direct durable mapping ambition remains post-release unless a misleading capability is found**.

Evidence:

- `tasks/list`/`_tasks/list`, `tasks/show`, `tasks/bind`, `external_tasks/import`, and `tasks/events` are routed through ACP extension calls.
- `_tasks/bind` checks `expected_revision` and returns structured `stale_revision`, `not_found`, `unsupported_source`, and `conflict` errors through `acp_plan_tasks::task_error`.
- Session imports are explicitly reported as `durability=session` with review required.
- Repo-durable binding is guarded by explicit stable IDs, explicit stable-id quality, and non-session source metadata before writing the task binding store.
- Runtime capabilities advertise the plan task contract with compatibility modes and explicitly report `pagination=false`.

Remaining risk:

- Capability payload now advertises `durable_bind=true` with `durable_bind_scope=repo_backed_explicit_stable_id_only`, matching the guarded implementation instead of implying every task can be durably bound.

### ACP session/config, diagnostics, and provenance follow-ups

Classification: **post-release unless capability inventory claims exceed shipped behavior**.

Evidence:

- `_session/status` and `_session/config` are not currently routed ACP extension methods. They also are not advertised in backend capability-surface metadata, so deferring them does not create a known capability-truthfulness bug.
- Runtime capability surfaces are exposed through `runtime/capabilities` and capability inventory/readiness extension calls.
- Extension and package inventory surfaces expose partial diagnostics/provenance: loaded/enabled/callable state, stability counters, last error metadata, config schema/values, secret readiness, package inventory, permissions, and capability health.
- `docs/acp-surface.md` explicitly does not pretend virtual Zed resources such as diagnostics are local files; richer virtual-resource diagnostics remain a future resource-fetch/unsupported-marker contract.
- The closeout should focus on truthfulness of advertised surfaces rather than expanding diagnostics breadth in 0.27.0.

Deferred follow-ups:

- Add explicit `_session/status` and `_session/config` only when a concrete ACP/Flynt client contract needs them.
- Expand diagnostics/provenance beyond capability inventory only if runtime/capabilities advertises a stronger diagnostic surface or a client consumes one.

### Current release-blocker assessment

No release-blocking ACP defect is proven from this triage pass. The strongest capability-truthfulness gap found in this pass was addressed by scoping advertised durable task binding to repo-backed tasks with explicit stable IDs.
