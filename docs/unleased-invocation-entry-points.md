---
id: "3e14e80a-03ad-4230-9110-2d2dcef6c50d"
title: "Unleased host invocation entry points"
status: exploring
tags:
  - "architecture"
  - "invocation"
dependencies: []
related: []
open_questions:
  - "Should every host-level execution entry point require accepted invocation admission?"
  - "Which principal, surface, and no-session-authority policy should bounded diagnostic probes use?"
  - "When should a hidden tool remain callable, and when should policy forbid execution?"
branches: []
---

# Unleased host invocation entry points

## Overview

Should every host-level execution entry point require accepted invocation admission?
This concern records an emerging question from the architecture review, not an implementation decision.
The proposed invariant is that host dispatch requires an admitted per-call lease with explicit authority and lifecycle handling.
Its diagnostic scope and policy remain open.

Descriptive alias: `unleased-invocation-entry-points`.
Record path: `docs/unleased-invocation-entry-points.md`.
The canonical design schema has no alias field, so the alias is recorded here.

The phrase “legacy bus” can obscure the boundary under review.
`EventBus` remains the active shared runtime for `Feature` integration, context, and events.
This concern addresses unleased host execution APIs while preserving the current bus and backend `Feature::execute` contract.

## Research

### Model references

Source model: [`site/diagrams/likec4/context.c4`](../site/diagrams/likec4/context.c4), view `hostComponents`.
Relevant element: `omegon.host.tools` (“Tool execution and policy”).
Relevant relationships:

- `omegon.host.orchestration → omegon.host.tools`: “Dispatches model-requested tool calls”.
- `omegon.host.tools → omegon.host.extensions`: “Invokes extension-owned tools”.

These references locate the concern in the observed model.
They do not imply that the diagnostic probes are model-requested calls.
The [working agreement](../site/diagrams/likec4/WORKING-AGREEMENT.md) requires concerns and accepted intent to remain distinguishable.

### Observed entry points — 2026-10-06 audit snapshot

[`core/crates/omegon/src/bus.rs`](../core/crates/omegon/src/bus.rs) exposes four unleased convenience entry points:

| Entry point | Observed callers |
|---|---|
| `execute_tool` | Five production call sites and 18 test call sites. |
| `execute_tool_with_sink` | Called by the `execute_tool` wrapper. |
| `execute_tool_with_context` | No callers found. |
| `execute_internal` | No callers found. |

These counts are dated evidence, not invariants.
All five production calls are in `execute_composition_probe`, at [`runtime_composition.rs:110–220`](../core/crates/omegon/src/runtime_composition.rs#L110).
The hidden operator CLI selects fixed, bounded probes rather than arbitrary tool names.
See [`main.rs:572–581`](../core/crates/omegon/src/main.rs#L572), [`main.rs:1385–1439`](../core/crates/omegon/src/main.rs#L1385), and [`main.rs:1566–1569`](../core/crates/omegon/src/main.rs#L1566).

The audit did not establish arbitrary model, ACP, or HTTP exposure to these APIs.
It makes no exploit claim.
Comments that describe historical loop or ACP use are not evidence of current callers.

### Admission and lifecycle differences

The unleased methods dispatch to a feature without the per-call lease, acknowledgement, and generation admission used by admitted paths.
They also omit the common permission-policy admission step and have different cancellation and replay behavior.
Compare the methods at [`bus.rs:2567–2851`](../core/crates/omegon/src/bus.rs#L2567) with `execute_tool_with_lease` and [`invocation_service.rs`](../core/crates/omegon/src/invocation_service.rs).
For example, the leased timeout path cancels a child token before returning a timeout result.

`invoke_tool` is an admission helper, not the complete model-batch permission and redaction pipeline.
Its request currently supplies no permission policy or role and an empty permission-subject list.
Replacing probe calls with this helper alone would not establish the intended diagnostic authority policy.

The native extension adapter retries after reconnect when invocation metadata is absent.
With admitted invocation metadata, a transport failure produces `UnknownCompletionError` rather than an automatic replay.
See [`extensions/mod.rs:674–777`](../core/crates/omegon/src/extensions/mod.rs#L674).
The codescan probe calls use the specialized service path, so they do not establish exposure to this generic extension fallback.

### Candidate staged scope

If the admission invariant is accepted, a possible sequence is:

1. Remove the two unused convenience entry points after confirming their caller inventory.
2. Define a bounded diagnostic scope, then migrate the composition probes and affected tests.
3. Retire the remaining unleased wrappers and helpers, then narrow visibility of leased backend dispatch.
4. Verify denied, disabled, and unavailable results, cancellation, acknowledgement, and uncertain-completion behavior with focused regressions.

This sequence is a proposal for review, not an implementation task list.
Probe migration must account for the existing typed `service:disabled` and `service:unavailable` results.
The bus and backend `Feature::execute` remain part of the candidate design.

## Decisions

### Host execution admission boundary

**Status:** open

**Rationale:** A common admission boundary could make authority, generation checks, cancellation, and completion handling consistent across host entry points. The observed production callers are bounded diagnostics. Review must decide whether the invariant applies to every host-level execution entry point and define any diagnostic exception explicitly.

### Diagnostic authority and tool visibility

**Status:** open

**Rationale:** The diagnostic principal, surface, and policy without session authority are unresolved. Model visibility and execution permission represent different concerns. Review must distinguish a hidden but callable tool from a forbidden tool and specify how diagnostic probes observe denied, disabled, and unavailable outcomes.

## Open Questions

- Should every host-level execution entry point require accepted invocation admission?
- Which principal, surface, and no-session-authority policy should bounded diagnostic probes use?
- When should a hidden tool remain callable, and when should policy forbid execution?

## Implementation Notes

### Constraints

- Keep this node `exploring` until review resolves the admission and diagnostic policy questions.
- Preserve the active `EventBus` runtime and backend `Feature::execute` contract in the candidate scope.
- Do not create issues, change tasks, or OpenSpec artifacts from this concern before review.
