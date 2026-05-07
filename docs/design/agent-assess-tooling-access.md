+++
id = "d6379bdb-594c-43c6-b583-5e011961efe1"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Agent harness access to /assess tooling

## Overview

Enable the agent harness to invoke lifecycle-safe slash commands through a generic bridge instead of requiring operator-only `/assess` steps. The bridge stays intentionally narrow: commands must opt in explicitly, publish a structured result contract, and declare their side-effect class before the agent can invoke them.

## Research

### Why this gap matters

The current lifecycle expects the agent to propose, spec, implement, and then run `/assess spec` or related review commands before archive, but the harness does not expose `/assess` as an invokable tool. That creates a workflow break where the agent can prepare everything but must hand control back to the operator for a command-only step, weakening autonomy and lifecycle reconciliation.

### Preferred direction

The operator prefers a platform-level bridge for slash commands rather than a one-off `/assess` tool. The bridge should support agent execution of approved slash commands with structured machine-readable results, explicit safety controls, and compatibility with existing operator-facing command UX.

### Structured result shape

A bridged slash command returns a normalized envelope:

```ts
{
  command: string;
  args: string[];
  ok: boolean;
  summary: string;
  humanText: string;
  data?: unknown;
  effects: {
    filesChanged?: string[];
    branchesCreated?: string[];
    lifecycleTouched?: string[];
    sideEffectClass: "read" | "workspace-write" | "git-write" | "external-side-effect";
  };
  nextSteps?: Array<{
    label: string;
    command?: string;
    rationale?: string;
  }>;
  confirmationRequired?: boolean;
}
```

`humanText` still serves the operator-facing path, but the agent should key off `ok`, `data`, `effects`, `nextSteps`, and `confirmationRequired` first.

### Safety model

The bridge does not execute arbitrary slash commands by name. Commands opt in through explicit metadata:

- `agentCallable: boolean`
- `sideEffectClass: read | workspace-write | git-write | external-side-effect`
- `requiresConfirmation?: boolean`
- `resultContract?: string`
- `summary?: string`

If a command is not allowlisted, the bridge returns a structured refusal. If a command requires confirmation, the bridge returns `ok: false` plus `confirmationRequired: true` instead of silently executing.

### Shared implementation path

The bridge is useful only if interactive and harness execution cannot drift. Bridged commands therefore expose one structured executor, and the interactive slash-command registration renders from that result instead of owning separate business logic. In practice this means:

- the shared executor computes the command outcome once
- the bridge returns the structured envelope to the agent
- the interactive path renders or notifies from the same result

This is the core “implemented once, rendered twice” contract.

### Assessment-specific contract

`/assess` is the v1 proving ground because it blocks autonomous OpenSpec flows today. Structured assessment results should include:

- subcommand identity (`spec`, `diff`, `cleave`, `complexity`)
- status summary and operator-facing `humanText`
- command-specific data like scenario counts, chosen diff ref, or complexity decision
- lifecycle hints for pass vs reopen vs ambiguous follow-up
- suggested next steps the harness can act on without scraping prose

For lifecycle-aware assess flows, structured data must be sufficient for the agent to decide whether to call `openspec_manage.reconcile_after_assess` and with which outcome.

### OpenSpec lifecycle integration emphasis

The bridge should not stop at making `/assess` invokable. OpenSpec and cleave workflows depend on assessment as a state transition mechanism: spec verification, diff review, cleave review, and reconciliation after reopened work. That means bridged assessment results should remain first-class inputs to OpenSpec lifecycle handlers, and lifecycle-oriented slash commands should be designed to compose cleanly with assessment outcomes instead of treating `/assess` as an external/manual checkpoint.

## Decisions

### Decision: Build a general harness bridge for slash commands, not a one-off `/assess` shim

**Status:** decided
**Rationale:** The underlying gap is broader than assessment. If the harness can only invoke `/assess`, similar lifecycle breaks will recur for other command-only capabilities. A general slash-command bridge with explicit safety boundaries and structured result capture solves the platform problem once and lets agent workflows invoke approved commands without bespoke wrappers for each one.

### Decision: Slash-command bridge should be allowlisted and return structured results

**Status:** decided
**Rationale:** A general slash-command bridge is only safe if commands opt in explicitly. Each bridged command should declare whether it is agent-callable, its side-effect class, and a machine-readable result schema. The harness tool should invoke only allowlisted commands and return a normalized envelope instead of raw terminal text.

### Decision: Assessment commands should expose a first-class structured result contract

**Status:** decided
**Rationale:** The agent must be able to reconcile OpenSpec, design-tree, and follow-up fixes without scraping TUI-oriented prose. Assessment-capable commands should therefore return a structured payload including status, findings, severity summary, suggested next steps, changed file hints, and lifecycle reconciliation signals, while still preserving a human-readable rendering for interactive use.

### Decision: Human-readable command UX and agent execution should share one implementation path

**Status:** decided
**Rationale:** Duplicating command logic into separate slash-command and tool-only implementations would create drift. Commands should execute through shared internal handlers that produce structured results; the interactive slash-command path renders those results for humans, while the harness bridge returns the structured envelope directly to the agent.

### Decision: V1 should prioritize lifecycle-critical commands while keeping the bridge generic

**Status:** decided
**Rationale:** The platform should be generic, but the first commands onboarded to the allowlist should be the ones blocking autonomous workflows today: `/assess spec`, `/assess diff`, `/assess cleave`, and other lifecycle-critical commands that already participate in OpenSpec/design-tree reconciliation. This keeps scope controlled while avoiding a dead-end `/assess`-only solution.

### Decision: OpenSpec and cleave commands should compose around structured assessment results

**Status:** decided
**Rationale:** Assessment is not just a convenience command; it is part of the lifecycle state machine for spec-driven work. Commands such as `/opsx:verify`, `/opsx:archive`, reconciliation flows, and future cleave lifecycle helpers should consume structured assess outcomes directly so the implementation workflow remains autonomous, machine-readable, and internally consistent.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/lib/slash-command-bridge.ts` (new) — shared allowlist metadata, normalized bridge result envelope, refusal/confirmation behavior, and bridge-owned tool wrapper
- `extensions/types.d.ts` (modified) — augments registered commands with bridge metadata and structured executor typing
- `extensions/cleave/assessment.ts` (modified) — defines structured `/assess` result shapes and lifecycle hints
- `extensions/cleave/index.ts` (modified) — exposes shared structured assess executors used by human `/assess` rendering and future bridge wiring
- `extensions/openspec/index.ts` (modified) — accepts structured post-assess reconciliation inputs instead of prose-only review handoffs
- `extensions/design-tree/index.ts` (modified) — remains available for structured implementation-note updates when reconciliation expands file scope or constraints
- `extensions/lib/slash-command-bridge.test.ts` (new) — regression coverage for allowlist refusal, confirmation gating, tool wrapper metadata, and shared executor parity
- `docs/agent-assess-tooling-access.md` (modified) — documents bridge architecture, safety boundaries, result envelope, and rollout constraints
- `extensions/cleave/bridge.ts` (new) — pure adapter that preserves full bridged slash-command args while mapping structured /assess results into the generic bridge envelope
- `extensions/cleave/bridge.test.ts` (new) — regression coverage for preserving full bridged args in the /assess result contract
- `extensions/openspec/spec.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `extensions/openspec/lifecycle-integration.test.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `extensions/openspec/reconcile.test.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes

### Constraints

- Assessment entrypoints exposed to the agent must return structured machine-readable results, not require scraping terminal-only output.
- Do not expose arbitrary slash-command execution unless explicit safety boundaries, allowlists, and side-effect semantics are defined.
- The v1 solution should cover the OpenSpec lifecycle gap first: spec assessment and cleave/diff review paths used by the agent workflow.
- Assessment access should preserve existing operator-visible UX while enabling autonomous execution inside the harness.
- Bridged commands must be implemented once and rendered twice: structured result for agents, human text for interactive users.
- Every bridged command must declare an explicit result schema or typed data shape; opaque string-only success responses are insufficient.
- The bridge must refuse commands that are not explicitly allowlisted as agent-callable.
- Commands with destructive or external side effects must surface confirmation requirements through structured metadata rather than silently executing.
- Arbitrary slash-command execution remains disallowed in v1 even though the bridge primitive is generic.
- Assessment entrypoints exposed to the agent must return structured machine-readable results, not require scraping terminal-only output.
- Do not expose arbitrary slash-command execution unless explicit safety boundaries, allowlists, and side-effect semantics are defined.
- The v1 solution should cover the OpenSpec lifecycle gap first: spec assessment and cleave/diff review paths used by the agent workflow.
- Assessment access should preserve existing operator-visible UX while enabling autonomous execution inside the harness.
- Bridged commands must be implemented once and rendered twice: structured result for agents, human text for interactive users.
- Every bridged command must declare an explicit result schema or typed data shape; opaque string-only success responses are insufficient.
- The bridge must refuse commands that are not explicitly allowlisted as agent-callable.
- Commands with destructive or external side effects must surface confirmation requirements through structured metadata rather than silently executing.
- OpenSpec and cleave lifecycle commands should consume structured assess outputs directly instead of reparsing human-readable assessment text.
- Bridged slash-command result args must preserve the original tokenized invocation; subcommand-specific fields in data are supplemental and cannot silently replace args.
- Bridged slash-command results must preserve original tokenized args; command-specific metadata in data is supplemental.
