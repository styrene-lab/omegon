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
