# Maintenance and revalidation design — planned

## Entry, ownership, and ordered delivery

The implementation baseline includes schema 14 and the accepted Wave 5 foundation.
Do not reserve a future migration number in this plan. The schema owner allocates
it against the integrated schema when implementation begins.

Wave 6 owns source-independence, equivalence, conflict, and correction admission.
Before correction integration, record its accepted contract reference, integrated
commit, DTO version, and relevant verification evidence. These are pending, not
satisfied by the existence of `feat/memory-wave6-reconciliation`.

| Group | Owner and gate |
|---|---|
| 7A policy | Memory domain owner freezes meanings, memory-kind classification, DTOs, transport matrix, and atomic write-set before migration. |
| 7B source | Host reader/scheduler owner develops bounded source snapshots against frozen DTOs; no correction admission. |
| 7C atomic state | One exclusive shared-types/schema owner extends `types.rs`, backend contracts, both backends, and receipt persistence after 7A. |
| 7D runtime | Main-crate owner integrates scheduling and provider calls through `memory_service.rs`; correction requires accepted Wave 6 integration. |
| 7E selection/transport | Domain owner coordinates eligibility, cache deadlines, retained history, migration, and adapters against frozen 7A/7C contracts. |
| 7F evaluation | Evaluation owner verifies quality, negative cases, concurrency, reopen, and handoffs after relevant slices integrate. |

These are ordered integration groups, not authorization for concurrent edits to
shared files. Pure policy characterization, source readers, and fixtures can run
in parallel after DTO freeze. Shared types and schema have one exclusive editor.
Wave 8 requires the accepted evidence-versus-attention/validity contract slice and
its verification reference. It need not wait for unrelated scheduler completion.

## 7A: Policy before migration

Freeze a versioned policy table and wire meanings before schema or behavior changes.
The classification is explicit and inspectable; `Section::Architecture` alone is
not evidence that a fact is a durable invariant.

| Dimension | Planned meaning |
|---|---|
| Memory kind | Durable invariant, time-limited guidance, episodic observation, or legacy/unknown. Classification records its origin; absent evidence remains unknown. |
| Evidence confidence | Support under the accepted evidence/admission policy, with attribution. Neither numeric confidence `1.0` nor a usage/reinforcement count means verified. |
| Freshness | Separate observed-source time, last completed check, and last successful claim verification. Attempt time and failure reason belong to work records. |
| Usage/attention | Local recall/reference/duplicate-use signals may affect ranking, never evidence strength or verification time by themselves. |
| Validity | Existing platform/workspace/revision/component constraints and inclusive `valid_from`, exclusive `valid_until`. Unknown scope remains unknown. |
| Retention | Active, dormant, archived, superseded, and other retained states remain lifecycle decisions, not truth scores. No automatic deletion from failed reads. |

An old supported invariant remains current when applicable. Time-limited guidance
uses declared validity or an explicit review condition. An episodic observation
retains its historical attribution without becoming general guidance. Unknown
legacy facts retain their metadata but gain no invented support or expiration.
Freeze default review conditions and ranking rules per kind using development
fixtures before rollout; age alone must not revoke support.

`last_checked` means a complete supported-source check, not an attempted read or
successful embedding job. It can advance on a complete readable-but-unverified
check. `last_verified` advances only for an admitted claim-verification outcome.
Unchanged hashes may establish source continuity but cannot establish verification
that never occurred. Changed-back content requires a new check against the queued
source generation; hash equality must not erase intervening uncertainty.

Current reads and transport already have no-reinforcement coverage. Preserve and
characterize that behavior. Remaining write-side work includes vault `related_facts`
through `ReinforceFactOnce`, `facts_reinforced` reports, `reinforce_references`
configuration, and duplicate `StoreFact` paths that update reinforcement clocks.
Consume Wave 6's source-independence decision for duplicate evidence. Freeze the
configuration/report compatibility mapping and update operator guidance, including
the root directive about resetting decay, in the later implementation change.

## 7B: Source work and uncertainty

Source inspection is not revalidation. `indexing.rs` tracks embedding attempts,
which is also distinct from evidence checking. Reuse its attempt/precondition
patterns where appropriate, not its success state as a verification result.

The host owns filesystem/provider access. Domain code owns provider-neutral work
and admission contracts. Freeze these scheduler details before implementation:

- Triggers: observed supporting revision/hash changes, explicit review requests,
  and per-kind review deadlines. Startup/reopen resumes outstanding work.
- Stable work identity: workspace/mind, canonical source identity, observed source
  generation, fact ID/version, and policy version. Attempts have distinct IDs.
- Fanout: enumerate only linked facts with bounded keyset pages and a stable
  watermark. Persist the cursor; late arrivals belong to the next pass.
- Cadence and fairness: configured scan cadence, per-source/per-mind quotas,
  deterministic fair rotation, and capacity admission without silent drops.
- Capacity: explicit queue depth, page size, fanout, files, bytes, provider tokens,
  concurrency, pass deadline, and attempt deadline. Report deferred work.
- Recovery: bounded backoff with a cap and retry classification; injected clock
  and deterministic jitter in tests. Retry does not create duplicate active work.
- Cancellation: check before/after reads, between pages, before provider requests,
  and before mutation admission. Persist resumable progress and terminal reasons.
- Preconditions: exact fact version and source snapshot identity/generation at
  completion. A newer correction or replaced source attempt rejects stale work.

Choose and record concrete new limits in 7A/7B rather than call the scheduler
bounded without values. Preserve existing ceilings or explicitly justify tighter
limits: vault snapshots have 10,000 files, 8 MiB/file and 64 MiB aggregate limits;
the lifecycle reader has a 1 MiB artifact limit. Managed fact pages cap at 1,000.
These are existing owner-specific limits, not one interchangeable global budget.
Filesystem cancellation is cooperative; it is not a hard kernel-I/O interrupt.

| Source outcome | Knowledge and work effect |
|---|---|
| Complete, verified support | Admit support through the frozen evidence contract and advance verification attribution. |
| Changed or changed-back source | Record the observed generation and revalidate linked claims; no automatic confirmation or correction. |
| Readable but unverified | Record completed check and unresolved support; preserve prior verification time. |
| Missing, denied, unsupported host/source, or path escape | Record unavailable/unsupported reason; preserve evidence and prior checked/verified timestamps. |
| Oversized, truncated, cancelled, budget exhausted, or provider outage | Record incomplete attempt and retry/terminal classification; preserve uncertainty and prior checked/verified timestamps. |
| Complete contradictory evidence | Submit a correction candidate through accepted Wave 6 admission; retention is a separate decision. |

An incomplete source read cannot produce a negative finding, retirement, deletion,
or a refreshed verification timestamp. Diagnostics expose bounded reason codes,
counts, source identifiers, and progress, not fact or source contents.

## 7C: Atomic maintenance and replay

Extend `maintenance.rs`; do not create a separate maintenance storage owner.
`DormancyPlan` already provides deterministic sorted dry runs but carries IDs
without `FactPrecondition`, operation identity, or receipt binding. Its second
application currently returns zero transitions, not the first outcome.

Freeze a canonical plan payload with policy version, operation ID, reasons,
expected fact versions, source snapshot preconditions, intended transitions,
evidence updates, queue completion, and outcome summary. Freeze the atomic
write-set: all targeted fact/version changes, evidence/check records, lifecycle
edges where applicable, selection invalidation, work completion, and receipt.

Choose whole-plan atomic admission: any stale fact/source precondition rejects all
writes in that plan. Bounded independent pages are separate operations, never an
unreported partially applied plan. A preflight rejection leaves no success receipt.
Commit success and its outcome receipt atomically. Document how conflicts are
reported without claiming a committed transition.

Use `MemoryBackend::mutation_receipt` with the canonical payload hash before any
fresh version lookup or source read. Identical replay returns the original effect,
including after reopen or source removal. The same operation ID with a different
payload is a conflict. Reuse `apply_mutation_bound`, `FactPrecondition`, and
`MemoryMutation::TransitionFacts`; extend the existing mutation vocabulary only
where the frozen multi-record write-set needs it. Both backends share admission.

## 7D: Managed runtime

All durable work flows through the managed writer in
`core/crates/omegon/src/memory_service.rs`. Host orchestration performs source reads
and optional provider calls outside domain/storage transactions, then submits
bounded results for atomic admission. Providers do not enter `omegon-memory`
domain code. Provider unavailability does not disable storage or lexical reads.

Use the secure lifecycle reader in `features/memory/lifecycle.rs` and vault snapshot
boundary as reference implementations, preserving supported-host and path rules.
Register review/apply/status/cancel operations through existing command ownership
and `CommandDefinition` metadata for applicable TUI/CLI/ACP surfaces. Expose pending,
running, deferred, unavailable, conflict, and completed outcomes consistently.

## 7E: Selection, retained history, and transport

Change `effective_confidence`, `ambient_score`'s `0.10` exclusion, dormancy policy,
and `selection_cache.rs`'s `confidence_floor_deadline` use together. Otherwise an
old supported fact can survive one path and disappear from another. Cache keys and
deadlines must include frozen policy/support revisions and validity boundaries.
Preserve the existing 30-second rank-freshness ceiling unless explicitly amended.

Keep `SearchIntent::Historical`'s archive population: Archived, Dormant, Superseded.
Add an explicit compatible all-status retained-history query mode. It includes
Active-but-expired facts and labels pending/unverified records without promoting
them. With an explicit as-of context it assesses existing applicability at that
time, using inclusive-from/exclusive-until boundaries. Without as-of time it returns
retained history with unknown temporal applicability rather than forcing current
time. This is valid-time assessment of retained records, not reconstruction of a
complete past database snapshot. Apply scope/status filtering before truncation
across lexical, vector, lookup, graph, and public adapters. Do not archive a fact
merely to make it searchable. Historical ranking bypasses current age exclusion.

Allocate migration against the schema present at implementation start, retaining
the observed schema-14 store as a compatibility fixture. Preserve absent legacy
fields as unknown and distinguish declared imported evidence from local checking.
Freeze exact DTO names and the following field policy before migration:

| Fields | Portable/import policy |
|---|---|
| `id`, `mind`, `content`, `section`, `status`, `created_at`, `version` | Preserve existing identity, lifecycle, and version/merge semantics. |
| `source`, `content_hash`, `supersedes`, applicability and `recorded_at`, lifecycle inference | Preserve attribution, links, constraints, and uncertainty without granting local verification. |
| `decay_profile`, `confidence`, `reinforcement_count`, `decay_rate`, `last_reinforced` | Preserve individually as legacy metadata. Do not reinterpret defaults or `1.0` as calibrated evidence. |
| Existing `last_accessed` | Preserve historical transport behavior; do not silently delete it because new usage state is local. |
| `created_session`, `superseded_at`, `archived_at`, `jj_change_id` | Preserve existing historical metadata and absence semantics. |
| `persona_id`, `layer`, `tags` | Preserve existing ownership/classification metadata; tags alone do not verify memory kind. |
| New memory kind, evidence support, source observations, checked/verified history, validity/review declarations | Portable with origin and policy version. Imported verification remains attributed imported verification, not a locally completed check. |
| New local usage counters, salience, local access/reference clocks | Persist locally; exclude from portable evidence transport. Import does not reset or replace destination attention. |
| Queue IDs, attempts, cursors, backoff, leases, deadlines, local check progress, receipts | Local durable execution state; exclude from portable transport. Import neither resumes foreign work nor overwrites destination progress/receipts. |

Legacy operational-field absence must not erase known metadata on an existing
record. Explicit values retain established import validation/merge behavior.
Round-trip fixtures cover both backends, legacy and modern JSONL, reopen, and
bidirectional vault idempotency. A portable verification record is not a local
receipt and cannot satisfy local work completion by itself.

## 7F: Evaluation and acceptance

Use a fresh synthetic corpus contrasting stale guidance with old still-supported
knowledge. Include expired Active records, unknown legacy facts, repeated usage,
copied references, changed-back sources, incomplete reads, and false corrections.
Measure current-guidance precision, old-supported retention, false verification,
false retirement/correction, retrieval coverage, and bounded execution cost.

Freeze numerical development thresholds, corpus IDs, policy version, and budget
before a held-out run. Keep labels inaccessible to the writer/retriever. Report
negative cases and per-kind results, not only an aggregate score. The prior Wave 5
model comparison does not establish this policy's quality or authorize further
spending. Obtain a fresh bounded authorization before any live-model evaluation.

Record characterization separately from new behavioral red/green evidence. Run
both backend contracts, reopen/migration failure atomicity, source boundaries,
managed-service integration, and applicable landing gates. OpenSpec validation
establishes document structure only. Acceptance requires integrated evidence and
explicit Wave 6/Wave 8 contract handoff references.
