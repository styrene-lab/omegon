# Unified model request preparation contract

Implementation authorized by the user after PR #246 merged at
`1828d6c5a6f78eceae9e811e9ba39e4ccf4f59d4`. The accepted direction is a validated
internal request envelope over the existing route service, covering all three
flows below. Independent code review found no blockers. PR #247 merged the
implementation at `13760f256662de5cbb8089d38990f7db3c93d89d`.

## Post-merge acceptance — 2026-10-07

The user subsequently authorized landing and lifecycle and architecture
reconciliation. The shared internal envelope and all three implemented flows are
accepted. [Verification](verification.md#post-merge-acceptance--2026-10-07) records
the original tests, later provider-head regression evidence, and exact
`77996814` headless acceptance. Exact-head CI remains blocked by a Rust-build
timeout; these results do not validate a later rebased TUI tip.

All 12 implementation tasks remain checked. Native administrative closure is
pending, and the change is not archived. The
[closure concern](../../../docs/lifecycle-closure-reconciliation.md) explains why
standalone archive readiness is insufficient for native ledger reconciliation.

## Historical proposal — 2026-10-07, before implementation

The problem statement, candidate vocabulary, and plan below retain their original
pre-implementation perspective. Post-merge acceptance above supersedes that
perspective where it describes work as proposed.

## Intent

Give normal turns, compaction, and bounded auxiliary inference a shared request
preparation contract. Make common invariants inspectable without requiring every
caller to own an interactive session or use the interactive retry loop.

The intended benefit is fewer places where request identity, provider-visible
inputs, route evidence, tool restrictions, and flow-specific limits can disagree.
The proposal consolidates preparation around existing owners rather than creating
another provider gateway.

## Problem and evidence

Omegon already shares provider resolution through `ProviderRouteServiceContract`
and captures route evidence before dispatch. Authority-backed turns also record
exact context and schema manifests through `ModelRequestPrepared`.

Preparation remains distributed across three observed paths:

- Normal turns assemble context, select tools, validate capacity and route
  capabilities, capture authority evidence, and construct dispatch arguments.
  Context-overflow and history repair repeat parts of request capture and dispatch.
- Compaction builds a separate summary request with no tools. It has dedicated
  authority evidence and distinct turn-owned and idle-session ownership.
- Memory extraction calls `quick_completion_bounded`. It uses attributed session
  evidence, but dispatches through a sessionless step route recorder. Its caller
  owns a 30-second extraction deadline and bounded result validation.

These paths share concepts, not identical semantics. A common gateway already
exists; a common preparation envelope covering these constraints does not appear
in the inspected paths. This is a maintainability opportunity, not evidence that
all current requests bypass policy or that consolidation improves performance.

[Research and source references](design.md) distinguish existing contracts from
candidate changes. The linked [design concern](../../../docs/unified-model-request-contract.md)
records the architecture question. OpenCode V2's pinned
[shared preparation entry points](https://github.com/anomalyco/opencode/blob/9c51d84f8f5284e4ec9a1291e8fbed9be261d5f1/packages/core/src/session/model-request.ts#L203-L250)
motivate the review. Its session-bound API and hook behavior are not proposed as
Omegon's contract.

## Scope

Approved first scope:

1. Normal turn preparation, including context-overflow and history-repair requests.
2. Summary-request preparation for turn-owned, idle-session, and compatibility compaction.
3. Bounded auxiliary completion, with memory extraction as the first acceptance case.
4. Shared validation and evidence linkage immediately before existing dispatch paths.

Excluded from this proposal:

- Replacing `ProviderRouteServiceContract`, provider adapters, session authority,
  context selection, compaction planning, or the tool invocation permission owner.
- Sending auxiliary inference through the interactive loop, constructing synthetic
  turns, or assigning session authority from a source-session label.
- Unifying retry counts, timeouts, output limits, prompts, tool sets, or result parsers.
- Adding title or general-generation flows because a peer has them. Sentry's
  existing unbounded helper callers need separate compatibility decisions.
- New public APIs, provider hook systems, provider/authentication policy changes,
  transcript migrations, or a new persisted request format by default.

## Candidate request envelope

This is a review vocabulary, not a proposed Rust type or serialized schema.

| Concern | Candidate content and invariant |
| --- | --- |
| Purpose and identity | Flow kind, logical request identity, repair relationship, and execution owner. Use distinct variants for turn, idle compaction, and sessionless work. |
| Inputs and provenance | Ordered system/messages, source attribution, and available immutable references. Exact authority-backed content must match what dispatch consumes. Auxiliary source attribution does not confer write authority. |
| Route binding | Captured execution binding where applicable, selected/serving identities, provider options, capability evidence, dialect, and generation. Existing route service retains resolution, authentication, and lease ownership. |
| Tool surface | Captured definitions plus owner/composition lineage, or explicit no-tools. Preparation cannot grant execution permissions or add tools beyond the caller's admitted surface. |
| Limits | Explicit flow policy for context capacity, reply reserve, output bytes/tokens, deadline, idle timeout, and retry behavior. Preserve units and distinguish provider defaults from host-enforced limits. |
| Cancellation and completion | Owner-supplied cancellation/cleanup policy and terminal requirements. Preparation must not manufacture completion from future destruction or partial output. |
| Evidence and observation | Existing request/route/attempt or compaction identities, sessionless step evidence, and bounded diagnostics. Keep credentials and restricted continuity out of ordinary telemetry. |

An envelope should reference existing authority and policy objects where possible.
Common validation can run once at a defined boundary. Flow adapters still prepare
their domain inputs and execution owners still consume results, retry, cancel,
commit, and publish them.

## Success criteria

The delta scenarios specify these acceptance conditions. Verification evidence is
recorded separately in `verification.md`.

| Flow | Measurable outcome |
| --- | --- |
| Normal turn | For fixed inputs, provider-visible context, ordered tool schemas, route identity, and generation match the pre-migration capture. Oversized mandatory context fails before dispatch. A repair creates a new request and lease in the same step; an unchanged-route transport retry retains its request identity. |
| Tool-bearing turn | Tools disabled at capture are excluded. Later tool additions do not mutate the frozen request surface. Final-response-only requests advertise zero tools. Invocation-time permissions and revocation remain authoritative even if a tool was advertised. |
| Turn-owned compaction | Dispatch has zero tools and matching compaction request/route evidence. Oversized authority input fails without truncating its recorded meaning. EOF, timeout, provider failure, or empty output cannot commit a summary. |
| Idle compaction | Preparation preserves the session-scoped compaction evidence and admission owner. It creates no prompt, turn, or loop step. Compatibility compaction retains its separate sessionless behavior. |
| Memory extraction | Dispatch has zero tools and a step-owned route record. The current 30-second extraction deadline, `MAX_EXTRACTION_BYTES`, and explicit Done requirement remain effective. Invalid candidates remain rejected. Stale source generation or cancellation cannot publish a successful extraction. |
| Every included flow | Invalid ownership, unsupported capabilities, or failed required evidence writes prevent provider dispatch. Selected and serving identities remain distinguishable. No flow inherits broader credentials, tools, retries, or data retention from another flow's defaults. |

## Compatibility and migration approach

Start with an internal preparation contract in the integration owner. Evaluate
whether shared traits need vocabulary only after actual cross-crate consumers are
identified. Keep captured session execution generations and quiescent replacement
rules intact.

Characterize current requests with fake bridges and authority fixtures before
changing call sites. Adapt one flow at a time through existing route-service
entry points. Compare prepared inputs and evidence without issuing duplicate live
provider calls. Retain flow adapters until parity is demonstrated.

The initial migration should preserve persisted v1 facts and replay semantics.
Any necessary new durable fact or changed sessionless evidence is a separate,
explicit compatibility decision. Existing baselines for
[route leases](../../baseline/provider-routing/leases.md) and
[session authority](../../baseline/runtime-session/authority.md) remain constraints.

## Test and acceptance plan

- Use deterministic fake bridges to inspect final messages, options, tool sets,
  dispatch count, and rejection-before-dispatch behavior for each included flow.
- Use authority/replay fixtures to compare context manifests, schema lineage,
  route joins, repair identities, idle compaction evidence, and terminal outcomes.
- Exercise cancellation before preparation, during dispatch, and during retry or
  result collection. Assert bounded owned cleanup and truthful terminal evidence
  at the responsible owner, including memory generation changes.
- Inject capability rejection, evidence-write failure, input overflow, output-byte
  overflow, EOF without Done, timeout, and provider error. Assert no false commit.
- Verify existing provider/auth and tool-permission tests retain their outcomes.
  Validate legacy and sessionless paths without inventing authority facts.

The reviewed direction has delta scenarios and a bounded task plan. Implementation
validation uses the owning crate's focused tests and landing gates.

## Original review questions and implementation disposition

1. **Boundary:** The user accepted the internal validated envelope. Capability
   checks, no-tools validation, frozen inputs, and evidence ordering share it.
2. **First scope:** All listed flows are included. Unbounded helper callers keep
   their existing EOF behavior and use an explicit compatibility budget policy.
3. **Ownership:** Auxiliary preparation requires a step recorder. Attributed
   evidence stays in the prompt and memory formation, without session append authority.
4. **Limits and cancellation:** Only the auxiliary byte bound is consumed through
   the envelope. Other existing budgets, deadlines, cancellation, and collectors
   remain flow-owned. This migration does not add a unified execution abstraction.
5. **Evidence:** Existing manifests, route leases, and compaction evidence suffice
   for this migration. No additional durable fact or persisted-schema change is used.

The user accepted the envelope boundary and the complete first scope. Implementation
choices are recorded in design.md. Delta specs and completed tasks describe the
accepted implementation; baseline reconciliation remains pending native closure.
