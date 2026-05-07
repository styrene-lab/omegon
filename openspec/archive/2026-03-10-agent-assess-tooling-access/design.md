+++
id = "07d84be3-0776-407d-8d44-9be8d9d898d1"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Agent harness access to /assess tooling — Design

## Architecture Decisions

### Decision: Build a general harness bridge for slash commands, not a one-off `/assess` shim

**Status:** decided  
**Rationale:** The underlying gap is broader than assessment. If the harness can only invoke `/assess`, similar lifecycle breaks will recur for other command-only capabilities. A general slash-command bridge with explicit safety boundaries and structured result capture solves the platform problem once and lets agent workflows invoke approved commands without bespoke wrappers for each one.

### Decision: Slash-command bridge should be allowlisted and return structured results

**Status:** decided  
**Rationale:** A general slash-command bridge is only safe if commands opt in explicitly. Each bridged command should declare whether it is agent-callable, its side-effect class, and a machine-readable result contract. The bridge invokes only allowlisted commands and returns a normalized envelope instead of raw terminal text.

### Decision: Assessment commands should expose a first-class structured result contract

**Status:** decided  
**Rationale:** The agent must be able to reconcile OpenSpec, design-tree, and follow-up fixes without scraping TUI-oriented prose. Assessment-capable commands should therefore return a structured payload including status, findings, severity summary, suggested next steps, changed file hints, and lifecycle reconciliation signals, while still preserving a human-readable rendering for interactive use.

### Decision: Human-readable command UX and agent execution should share one implementation path

**Status:** decided  
**Rationale:** Duplicating command logic into separate slash-command and tool-only implementations would create drift. Commands should execute through shared internal handlers that produce structured results; the interactive slash-command path renders those results for humans, while the harness bridge returns the structured envelope directly to the agent.

### Decision: V1 should prioritize lifecycle-critical commands while keeping the bridge generic

**Status:** decided  
**Rationale:** The platform should be generic, but the first commands onboarded to the allowlist should be the ones blocking autonomous workflows today: `/assess spec`, `/assess diff`, `/assess cleave`, and adjacent lifecycle commands if needed. This keeps scope controlled while avoiding a dead-end `/assess`-only solution.

## Research Context

### Why this gap matters

The current lifecycle expects the agent to propose, spec, implement, and then run `/assess spec` or related review commands before archive, but the harness does not expose `/assess` as an invokable tool. That creates a workflow break where the agent can prepare everything but must hand control back to the operator for a command-only step, weakening autonomy and lifecycle reconciliation.

### Preferred direction

The operator prefers a platform-level bridge for slash commands rather than a one-off `/assess` tool. The bridge should support agent execution of approved slash commands with structured machine-readable results, explicit safety controls, and compatibility with existing operator-facing command UX.

### Structured result envelope

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

`data` holds command-specific structured output. For assessment commands this should include findings, severity counts, reviewed target, pass/reopen state, and lifecycle reconciliation hints such as whether `openspec_manage.reconcile_after_assess` is warranted.

### Safety model

The bridge does not execute arbitrary slash commands by name. Commands opt in through explicit metadata such as:

- `agentCallable: boolean`
- `sideEffectClass: read | workspace-write | git-write | external-side-effect`
- `requiresConfirmation?: boolean`
- `resultContract?: string`
- `summary?: string`

The bridge refuses commands that are not allowlisted as agent-callable. If a command requires confirmation for destructive or external side effects, the bridge returns a structured refusal/confirmation-needed result instead of executing silently.

### Shared implementation path

The cleanest architecture is to factor slash-command bodies into shared handlers that return structured results. Existing interactive command registrations become thin renderers over those handlers, while a harness-facing tool invokes the same handlers by command id. This avoids parsing terminal text, keeps command behavior consistent across human and agent entrypoints, and lets commands gradually opt into bridge support.

### Validation strategy

Regression coverage should explicitly check:

- allowlisted execution succeeds and preserves structured data
- blocked commands fail closed
- confirmation-required commands refuse execution until confirmed
- interactive and bridged paths observe the same structured executor output
- structured assess results are sufficient to drive lifecycle reconciliation without prose parsing

### V1 rollout

V1 should onboard the lifecycle-critical assessment commands first:

- `/assess spec`
- `/assess diff`
- `/assess cleave`

The bridge remains generic, but these commands close the current autonomy gap immediately. Additional commands can opt in later once they provide metadata and structured result contracts.

## File Changes

- `extensions/lib/slash-command-bridge.ts` (new) — shared bridge primitives for allowlisted slash-command execution, metadata lookup, normalization, and safety checks
- `extensions/types.d.ts` (modified) — command metadata/result typing support for bridged commands
- `extensions/cleave/assessment.ts` (modified) — structured assessment result types and lifecycle hints
- `extensions/cleave/index.ts` (modified) — shared `/assess` structured executors used by human and bridged flows
- `extensions/openspec/index.ts` (modified) — consumes structured assessment outcomes where lifecycle reconciliation is needed
- `extensions/design-tree/index.ts` (modified) — consumes structured reopen/update signals where needed
- `extensions/lib/slash-command-bridge.test.ts` (new) — regression tests for allowlisting, confirmation handling, and shared executor parity
- `docs/agent-assess-tooling-access.md` (modified) — records architecture, safety boundaries, result envelope, and v1 allowlist

## Constraints

- Bridged commands must be implemented once and rendered twice: structured result for agents, text rendering for interactive users.
- Every bridged command must declare an explicit result schema or typed data contract; opaque string-only success responses are insufficient.
- The bridge must refuse commands that are not explicitly allowlisted as agent-callable.
- Commands with destructive or external side effects must surface confirmation requirements through structured metadata rather than silently executing.
- V1 must cover the current OpenSpec lifecycle gap first by making `/assess spec`, `/assess diff`, and `/assess cleave` invokable through the bridge.
- Arbitrary slash-command execution remains out of scope for v1 even though the bridge primitive itself is generic.
