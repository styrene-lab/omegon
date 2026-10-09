# Unified model request contract — implementation design and research

## Implementation authority and provenance

The user accepted shared validated internal preparation after PR #246 merged.
Implementation owner: `feat/unified-model-request-preparation`, based on
`1828d6c5a6f78eceae9e811e9ba39e4ccf4f59d4`.

The proposal, this research ledger, and `docs/unified-model-request-contract.md`
were copied from the uncommitted draft in the sibling `omegon-likec4` worktree.
The historical research below retains its original revision and uncertainty.
No diagram assets were imported.

## Post-merge acceptance — 2026-10-07

PR #247 merged this implementation at `13760f25`. The user subsequently authorized
landing and lifecycle and architecture reconciliation. The internal envelope and
all three flows are accepted; independent review and the recorded author gates
passed. Later regression evidence covers the composed provider-repair head
`a08d526`, whose merge is `024a8616`, rather than the original request-contract head.
See [verification](verification.md#post-merge-acceptance--2026-10-07).

The parent architecture model now includes
`omegon.host.memory → omegon.host.inference: Requests bounded memory extraction`
in `hostComponents`. The shared preparation contract spans existing flow adapters
and needs no new component. The
[design node](../../../docs/unified-model-request-contract.md#architecture-model-references)
records model provenance and immutable implementation evidence.

## Implementation choices

- A main-crate `model_request` module owns an immutable, borrowed request envelope.
  It validates tool policy and capabilities before calling an owner-supplied
  evidence writer. Failed validation or evidence cannot produce a prepared request.
- Prepared inputs hold the original system, messages, and ordered tool slice and
  an options snapshot. Existing resolved routes alone substitute the native model.
  No new service, public trait, persisted event, or credential field is required.
- Final turn admission reads existing manifest blobs and rejects changed system,
  messages, schemas, normalizer generation, request ordinal, or step ownership.
  A durable request requires its response-fact owner before the first attempt.
  This adds a read-only equality check at dispatch, not a second durable capture.
- Turn capture uses one adapter for initial, overflow-repair, and history-repair
  requests. Authority still derives context and captures schema lineage. Repair
  supersedes once and gets a new identity; transport retries reuse preparation.
- Compaction and auxiliary adapters declare no-tools policies. Summary input
  selection, the 100,000-byte authority rejection/compatibility truncation split,
  120-second receive timeout, and explicit Done remain compaction-owned.
- Review found that compaction ignored structured `UpstreamFailure`. It now fails
  immediately, including when the stream contains a later Done event. This is the
  one collector correction needed to meet the approved provider-error criterion.
- Bounded completion carries its byte budget into the existing collector. Memory
  keeps its outer 30-second deadline, source generation checks, parser, cancellation,
  and publication. Source attribution grants no append authority. Unbounded helper
  callers retain their EOF compatibility behavior.
- A stale mutable memory capture is rejected before evidence admission. Already
  stored immutable evidence remains eligible for recovery, and finalization pins
  the ended session even after a UI session switch. The envelope does not reinterpret
  those source-generation rules or bind extraction to the current interactive turn.
- Captured session bindings and boot-bound auxiliary/manual-compaction bindings
  retain their existing owners. The envelope neither grants invocation permission
  nor replaces invocation leases.

## Review and lifecycle limits

These are implementation choices within the accepted direction. Scenario verification,
self-review, and independent code review are complete with no blocking findings.
The subsequent landing and reconciliation authorization supersedes the original
PR-creation-only handoff. Native lifecycle tools are not exposed in this session.
Factual artifacts are reconciled directly; native task/test registration, ledger
reconciliation, and archival remain pending. The design node retains its existing
`implementing` status to avoid claiming a tool-backed transition. The
[closure concern](../../../docs/lifecycle-closure-reconciliation.md) records the
late-registration and archive-semantics gaps.
The verification record names tests and gate outcomes. No provider-task cleanup stronger
than the existing receiver/owner contract is implied by dropping a future.

### Validation fixture correction

The full crate gate exposed an intermittent `WouldBlock` from the localhost
instruction-discovery fixture. Accepted sockets can inherit the nonblocking
listener flag on macOS. `tests/instruction_discovery_blackbox.rs` now explicitly
sets the accepted stream to blocking mode before its existing 15-second read
timeout. This test-only correction is separate from request preparation behavior;
its CLI deadline, request assertions, and process-group cleanup are unchanged.

## Historical proposal research — 2026-10-07, before implementation

The following research was drafted before implementation authorization. Source inspection used
Omegon commit `7b2da402073d84e54f3caffbb13845a007161c62` on
`chore/likec4-architecture-tooling`. The concurrent diagram work supplies model
references only. No runtime behavior was exercised for this proposal.

## Existing preparation and execution ownership

Paths below are relative to `core/crates/omegon/src/`. Symbol names are the stable
references; line ranges identify the inspected revision.

| Flow | Preparation and evidence | Execution owner |
| --- | --- | --- |
| Normal turn | `loop.rs:470–535`, `LoopContextCompatibilityAdapter::prepare_turn`, and `LoopSemanticFactAdapter::prepare_model_request` assemble context and capture the request. | The captured loop driver orchestrates through `LoopRoutePort`; the route service dispatches and collects the response. Host/session authority owns terminal commitment. |
| Repair request | `loop.rs:626–701` repeats request capture and dispatch after context or history repair. It supersedes the prior request while retaining the step. | Existing loop recovery policy and route-service execution. |
| Turn compaction | `loop.rs:332–430`, `LoopContextPort::begin_compaction`, and `compact_loop_route` prepare selected summary input and compaction evidence. | Context owner selects/applies the plan; route service collects a no-tools summary; compaction authority validates durable transitions. |
| Idle compaction | `control_runtime.rs:2818–2887` establishes idle authority, then calls boot-bound `compact_scoped` or compatibility `compact`. | Manual control path and compaction authority, not a fabricated interactive turn. |
| Memory extraction | `features/memory/formation.rs:18–38,435–494` prepares attributed evidence and invokes bounded completion. `providers.rs:767–803` builds the provider request. | Memory workers own deadline, cancellation/generation checks, parsing, and publication. Boot-bound provider resolution supplies a sessionless step-owned stream. |

Current request preparation is distributed across `loop.rs`, `loop_context.rs`,
`loop_session.rs`, and `loop_driver.rs`.

## Evidence ledger

### E1 — A common route gateway already exists

[`ProviderRouteServiceContract`](../../../core/crates/omegon/src/provider_route_service.rs#L433-L484)
exposes resolution, exact admitted resolution, startup/turn selection, preparation,
dispatch, and compaction. Its `prepare` currently performs route setup/warmup,
not the full model-request assembly proposed here.

[`SessionExecutionBinding`](../../../core/crates/omegon/src/session_execution.rs#L22-L156)
captures driver and route service together. Normal execution receives that service.
Auxiliary completion resolves through `boot_execution_binding`, and the inspected
manual compaction path also calls the boot binding. Shared service type does not
mean every flow shares one session's captured generation.

[`ResolvedProviderRoute::stream`](../../../core/crates/omegon/src/provider_route_service.rs#L403-L425)
validates admitted capabilities, records route ownership, and sets the native
model before streaming. [`resolve_provider_route`](../../../core/crates/omegon/src/provider_route_service.rs#L951-L999)
uses exact selection or declared compatible fallback and the existing credential
resolver. Exact inventory admission additionally distinguishes manifest-owned
secret bindings from compiled-provider factories at lines 496–587.

The proposal should reuse this authority. Authentication precedence, exact-route
admission, and fallback compatibility are not duplicated preparation policies to
replace. A prepared request can carry credential-source class, never credentials.

### E2 — Normal turns already capture exact inputs and tool lineage

[`LoopContextCompatibilityAdapter`](../../../core/crates/omegon/src/loop_context.rs#L72-L145)
resolves context windows and reply reserve, collects feature context, incorporates
attachment context, and composes the prompt. `validate_fixed_context` at lines
16–38 rejects mandatory instructions plus schemas and reply reserve that exceed
the provider window. It does not silently truncate mandatory instructions.

[`loop.rs:204–220`](../../../core/crates/omegon/src/loop.rs#L204-L220)
refreshes the advertised tool surface each iteration.
[`LoopInvocationPort`](../../../core/crates/omegon/src/loop_driver.rs#L1159-L1200)
chooses an empty surface for final-response turns and captures composition,
capability, contribution, and owner-generation lineage for advertised tools.
[`dispatch_batch`](../../../core/crates/omegon/src/loop_driver.rs#L1203-L1275)
separately enforces the dispatch limit and obtains current permission policy.
Advertising a tool is not permission to execute it.

[`prepare_model_request`](../../../core/crates/omegon/src/loop_session.rs#L499-L659)
rejects mismatched lineage length, a moved authority frontier, and messages that
do not byte-match authority-derived context. It materializes system provenance,
captures ordered message references, hashes context and schema manifests, and
validates continuity against the route. The existing
[`ModelRequestPrepared`](../../../core/crates/omegon/src/session_authority.rs#L306-L360)
already contains identity, purpose, replacement, continuity, context, and schema
fields. A new envelope should adapt these fields, not create a rival evidence record.

### E3 — Route evidence and response policy have established owners

[`record_loop_route_lease_for_request`](../../../core/crates/omegon/src/provider_route_service.rs#L244-L292)
joins authority-backed request identity to a durable route lease. It rejects
partial session scope and sessionless attempts to join a durable model request.
Pure sessionless dispatch uses `StepRouteLeaseRecorder`.

[`dispatch_loop_route`](../../../core/crates/omegon/src/provider_route_service.rs#L1459-L1573)
validates capabilities and records route evidence before the transport attempt.
It owns retry classification and limits; the stream consumer owns response
attempt evidence and phase-aware idle policy. The loop races initial dispatch
against cancellation at `loop.rs:519–706`. These policies are not equivalent to a
short auxiliary completion deadline.

Existing [route-lease baseline](../../baseline/provider-routing/leases.md) rules
require a new identity and lease after context repair, but retain the joined lease
for unchanged-request transport retry. They also preserve selected/serving identity,
restricted continuity, and quiescent driver/service replacement.

### E4 — Compaction is a distinct request and authority protocol

[`compact_loop_route`](../../../core/crates/omegon/src/provider_route_service.rs#L1287-L1456)
loads the summary prompt, constructs one user message, and supplies no tools.
It uses authority-selected input when available. Its constant is named
`MAX_COMPACTION_CHARS`, but the inspected bound uses UTF-8 byte length: 100,000.
Authority-backed oversized input fails; compatibility input is truncated at a
character boundary. Those behaviors must not accidentally merge.

Turn compaction records a turn-owned route lease before compaction preparation.
Idle compaction records equivalent route evidence through its session-scoped
compaction authority, without a turn lease. Summary collection uses a 120-second
per-receive idle timeout, requires Done, rejects empty output, and calls
`commit_done` only after success. That timeout is not an absolute request deadline.

[`LoopRoutePort::compact`](../../../core/crates/omegon/src/loop_driver.rs#L866-L885)
uses startup baseline options, while turn dispatch uses active options.
`LoopCompactionRequest` has no cancellation-token field. Pressure compaction in
the inspected loop is awaited before the ordinary dispatch cancellation select.
This is evidence of different boundaries, not proof of end-to-end cleanup behavior.
Cancellation ownership needs review before any shared execution abstraction.

The [session authority baseline](../../baseline/runtime-session/authority.md)
already defines compaction preparation, summary commitment, apply/abandon, and
idle admission semantics. A shared preparation contract cannot replace these with
ordinary assistant-message commitment.

### E5 — Memory extraction has session evidence but sessionless inference

[`CaptureSnapshot`](../../../core/crates/omegon/src/features/memory/formation.rs#L67-L126)
binds bounded semantic replay to a session-view generation and cancellation token.
The extraction prompt at lines 450–458 treats evidence as data, preserves
uncertainty, requires known evidence IDs, and keeps outputs as pending inferences.
The 30-second deadline and candidate parser are owned by `extract_candidates`.

[`quick_completion_internal`](../../../core/crates/omegon/src/providers.rs#L767-L855)
resolves a boot-bound route with no explicit secrets-manager argument, creates an
ephemeral step recorder, sends one user message with a classification system
prompt, and advertises no tools. Reasoning is disabled and extended context is
false. The bounded collector enforces the supplied byte limit and requires Done.
The helper has no cancellation-token or timeout parameter.

Memory cancellation is broader than this helper. See
[`checkpoint.rs`](../../../core/crates/omegon/src/features/memory/checkpoint.rs)
and [`finalization.rs`](../../../core/crates/omegon/src/features/memory/finalization.rs)
for worker cancellation and outer timeouts, and `formation.rs:383–432` for
content-free cancellation/readiness observation. A dropped caller future alone
does not demonstrate provider-task cleanup, so acceptance needs owner-level tests.

The unbounded `quick_completion` variant permits EOF return and is also called by
[`sentry/executor.rs:760,877`](../../../core/crates/omegon/src/sentry/executor.rs#L760).
That is a compatibility edge, not evidence that all auxiliary requests have the
same limits or terminal rules. These callers were located, not fully audited.

### E6 — Peer motivation, not a porting specification

OpenCode V2 at `9c51d84f8f5284e4ec9a1291e8fbed9be261d5f1`,
[`packages/core/src/session/model-request.ts:203–250`](https://github.com/anomalyco/opencode/blob/9c51d84f8f5284e4ec9a1291e8fbed9be261d5f1/packages/core/src/session/model-request.ts#L203-L250),
provides primary, compaction, generate, and title entry points through shared
preparation. Each keeps its flow hook. Primary/compaction use computed output
limits while title/generate keep provider defaults. The file also binds a tool
snapshot and filters hook-provided definitions against it.

The transferable idea is shared invariants with explicit flow variation. The
peer's required session input, hooks, transport selection, and execution closure
are not evidence that Omegon needs those APIs. This proposal contains no copied
implementation source and makes no claim about absent Omegon title/generate flows.

## Consolidation judgment

**Genuine repetition:** normal and repaired requests repeat capture/dispatch
argument construction. Turn, summary, and auxiliary paths each construct
provider-visible messages, options, tool surfaces, and evidence associations.
Their validity rules are reviewed across several owners rather than through one
explicit preparation boundary.

**Existing commonality:** route resolution, provider factories, authentication,
capability admission, schema dialect ownership, and lease construction already
have shared owners. Session requests already have a durable input manifest.
Unifying those again would add indirection without resolving the observed gap.

**Intentional differences:** context selection, summary payload selection,
memory-evidence selection, advertised tools, permission checks, budgets, retry
policy, cancellation, and authority commitments belong to different flows.
The candidate contract should make these inputs explicit and reject contradictions.
It should not turn them into permissive defaults.

## Candidate boundary and review constraints

Prefer flow adapters that produce a validated, immutable dispatch input consumed
by existing route entry points. Whether that input needs a separate preparation
service remains open. Shared checks should have one owner and flow-specific
checks should remain named and testable.

Preserve the following boundaries when specifying the candidate:

- Derive authority context and schemas before final capture; reject changed
  inputs rather than silently recapturing them under the same request identity.
- Keep source provenance distinct from execution authority, especially for memory.
- Keep the captured tool surface distinct from current invocation permission.
- Keep token estimates, output-byte bounds, idle timeouts, and absolute deadlines
  separate. A single numeric budget cannot represent them safely.
- Keep credential resolution and restricted continuity in their existing owners.
- Keep preparation evidence durable before dispatch where the existing flow requires it.
- Keep session generation capture separate from boot-bound auxiliary execution.
- Preserve truthful failure/cancellation outcomes before publishing success.

At draft time, no exact module split, trait signature, new durable event, or
migration ordering was decided. The implementation choices above supersede that
uncertainty for this first migration.

## Artifact and validation boundary

The path resolver selects this repository's `docs/` and `openspec/`; `ai/docs/`
and `ai/openspec/` are absent. The design concern uses only fields and sections
owned by `omegon-opsx/src/design_artifacts.rs`.

The original draft explicitly declared `proposed`. The implementation copy removes
that declaration so artifact-derived state follows its specs and tasks. Global
OpenSpec read-only validation checks structure but cannot validate Omegon ledger
reconciliation or prove behavioral acceptance. Closure is now authorized, but no
archive action has been performed. The source-backed closure concern above records
the remaining native limitation.
