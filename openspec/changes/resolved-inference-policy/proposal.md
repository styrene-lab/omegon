# Resolved inference route policy

## Intent

Resolve context capacity and reasoning once for the actual serving route. Bind
the result to request preparation and use the same snapshot in semantic UI.
Keep provider facts, operator intent, and effective request values distinguishable.

**Accepted implementation direction — 2026-10-08.** The user approved end-to-end
implementation and all three defaults. The [canonical design node](../../../docs/resolved-inference-policy.md)
is `implementing`. Task checkboxes track validated progress, not approval.

## Problem and evidence

At Omegon `6b276f3`, Settings can use a route maximum as its default window.
Context class and posture then select a smaller assembly budget. The TUI can
calculate usage against that assembly window and overwrite only the displayed
denominator with the registry window. Its conversation estimate also excludes
mandatory system and tool input.

Reasoning has a similar split: portable `Minimal` is stored and reserves 2,000
heuristic tokens, while Astra receives `low`. Explicit `Off` also becomes `low`.
A reasoning capability boolean cannot express route-specific effort sets or
whether reasoning can be disabled.

The [design evidence ledger](design.md#evidence-ledger) distinguishes these
observations from proposed norms and route-specific, dated comparison fixtures.

## Scope

- Resolve per-field capacity and reasoning evidence for an actual route and generation.
- Separate advertised default, documented ceiling, operator target, and host reserves.
- Validate full assembled input and final effective options before transport.
- Bind normal, repaired, compaction, and bounded auxiliary preparation to their own policy snapshots.
- Share semantic usage and reasoning projections, with details through existing `/context status`.

The implementation extends [shared request preparation](../unified-model-request-contract/proposal.md)
and its [preparation requirements](../unified-model-request-contract/specs/inference/preparation.md).
It preserves [route leases](../../baseline/provider-routing/leases.md),
[session authority](../../baseline/runtime-session/authority.md), and
[invocation authority](../../baseline/runtime-invocation/leases.md).
These remain separate authorities; the new deltas add policy and projection requirements.

Excluded: a new gateway or service, authentication or permission redesign,
universal discovery infrastructure, lower-level bridge elimination, transcript
migration, automatic profile rewriting, credentialed capacity stress tests, and
changes to the observed C4 topology. Existing flow-specific tools, collectors,
deadlines, cancellation, and completion ownership remain constraints.

## Recommended policy

1. Use the known route-advertised default. Larger context requires a positive
   numeric route-scoped override within known bounds. Resolve request, session,
   project, and user overrides in that order, rejecting an invalid winner.
   Unknown facts remain unknown; conservative host fallback is labeled `Assumed`.
2. Reject malformed capacity settings and unsupported concrete reasoning values
   before transport. Preserve portable intent and document supported normalization.
3. Reject `Off` when the route cannot disable reasoning. Saved incompatible
   values require operator resolution and are not silently rewritten.
4. Capture one policy with the prepared request. Late options cannot bypass it.
   Display actual effective reasoning and one coherent usage basis.

## Success criteria

- Default and maximum remain distinct in Codex Astra fixtures; API limits never
  become subscription-route facts by model-name matching.
- Prepared input, output accounting, route generation, wire reasoning, and UI
  agree, including repair, model switches, and sessionless auxiliary models.
  An increased enforced generation cap reduces a known total-window input budget.
- Usage count, percentage, and denominator come from one identified snapshot.
  Unprepared context has a budget without a fabricated full-request percentage.
- Invalid intent yields actionable, content-free diagnostics before any provider call.
- Offline fixtures cover the proposed deltas while retaining existing authority
  and flow contracts.

## Decisions accepted

The user accepted advertised defaults with numeric route-scoped enlargement,
strict unsupported `Off` rejection, and existing heuristic reserve values with
the defined equations. Final same-terminal runtime verification follows automated review.
