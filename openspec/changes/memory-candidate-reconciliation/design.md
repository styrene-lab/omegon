# Reconciliation design

Status: planned against `7b2da402`. [Assessment](assessment.md) separates existing
foundations from new contracts. Seed branch: `feat/memory-wave6-reconciliation`.

## 6A: Freeze contracts before implementation

`MemoryCandidate` currently contains only content, section, and evidence references
inside an immutable completed `EpisodeFormation`. It has no individual admission
identity or state. Lifecycle inference uses separate pending facts and a
snapshot-bound confirmation path that currently activates them directly.

Freeze the following decisions before assigning implementation owners:

| Contract | Required invariant | Still unresolved |
| --- | --- | --- |
| Candidate identity | Stable across retry, reopen, and supported transport; distinct from operation identity | Storage-key format, namespace, and compatibility for existing formations |
| Evidence identity | Reprocessing a source does not add independent support | Canonical source identity and rules for proving independence |
| Decision record | Bind candidate, evidence, applicability, authority, policy identity, target versions, outcome, and payload digest | Persisted format and concrete decision-record owner within the domain/backend boundary |
| Admission unit | Commit one frozen unit atomically; report partial progress only between units | Per-candidate versus related-candidate grouping and grouping identity |
| Refinement | Preserve justified specificity and history without inventing evidence or authority | In-place version versus linked replacement, and treatment of the broader claim |
| Confirmation | Bind approval to the reviewed decision and target versions | Extension of the existing snapshot/request contract and review presentation |
| Recovery | Separate extraction completion, durable decision, and admission completion | Work-state transitions, retry identity, and bounded scheduling ownership |

These are 6A decisions, not implemented types. Domain validation belongs in
`omegon-memory`; providers and operator surfaces belong in `omegon`. Freeze the
concrete decision owner and adapter boundaries without adding a second persistence
owner. Reuse `MemoryBackend`, SQLite and in-memory implementations, `MemoryMutation`
receipts, `FactPrecondition`, and `omegon/src/memory_service.rs`.

Declared provenance records what a producer claims. Independent support requires
validated source relationships. Different event IDs, extraction runs, operation
IDs, or paraphrases do not establish independence. Transported capture coverage
does not establish a local capture cursor or local admission receipt.

## 6B: Bounded matching and validated decisions

Use normalized hashes as a fast path, bounded lexical lookup, and optional semantic
signals. Include retired history in lookup to prevent paraphrase resurrection.
Filter by mind and applicability, but treat unknown scope conservatively rather
than as proof of equivalence or conflict. Different platform scopes can coexist.

A host-injected classifier proposes new, equivalent, refinement, correction, or
conflict outcomes. Domain validation checks evidence, scope, authority, and target
versions. Persist policy identity and decision inputs for replay. Unavailable,
malformed, or unsupported classification leaves typed pending work, not a merge.

Equivalence reuses a claim and preserves evidence references. Refinement adds only
supported specificity under the frozen 6A representation. Corrections require
applicable authority and preserve predecessor history. Recency alone cannot select
a truth winner. Conflicts retain an explicit unresolved relationship.

## 6C: Atomic admission through existing owners

An operation identity binds one immutable admission payload. Reuse with another
payload returns an operation conflict. Admission validates current target versions
and atomically commits fact changes, evidence and relationship edges, admission
completion, and its receipt. A failed precondition commits none of those effects.
Replaying a committed operation returns its recorded outcome without reinforcement.

The durable decision may precede admission. Its presence is not a success receipt.
Choose the next migration at implementation time: the operating schema at the
assessment revision is 14, not a reservation of the next schema number.

## 6D: Recover decision work separately from extraction

Existing recovery processes pending extraction. Extend the managed path with
bounded decision/admission recovery after 6A freezes ownership and transitions.
Resume a persisted decision after a crash before admission, revalidating versions
and approval. A lost acknowledgement replays the receipt. If some admission units
complete, retain that progress and retry only incomplete units. Never re-extract a
completed formation to reconstruct admission progress.

Cancellation and failures retain typed inspectable state. Imported decisions retain
provenance but cannot manufacture local operation receipts or confirmation.

## 6E: Host, confirmation, and read-side integration

Route foreground, extracted, and lifecycle inputs through common domain validation
with explicit intent. Preserve ordinary explicit-write behavior without blanket
approval. Preserve inferred lifecycle confirmation. For review-required decisions,
bind approval to candidate content, plan, evidence, applicability, authority, and
target versions. A changed plan or target requires renewed review.

Expose pending, admitted, conflict, and retired-history outcomes without presenting
unadmitted candidates as established knowledge. Update semantic context caches on
admission and relationship changes. Reject stale embedding/index completions for
old fact versions. Lexical operation remains available without embeddings.

## 6F: Evaluation and landing evidence

Wave 5 supplied a 32-row synthetic quotation smoke comparison with four held-out
cases. It did not measure semantic reconciliation quality. Build development and
held-out corpora for false merges, duplicate retention, corrections, authority
violations, and downstream task regressions. Include scope ambiguity, retired
history, and correlated evidence. Separate deterministic contract results from
classifier quality and downstream task results.

Measure development results, define metric denominators, and freeze numeric
acceptance thresholds, corpus/label digests, policy, prompts, models, and settings
before held-out evaluation. Held-out labels must not reach classifier inputs.
Record failures, uncertainty, unavailable calls, and measured costs separately.
Obtain a fresh live token/time budget and routing authorization; Wave 5's 100,000
tokens and 600 seconds are historical authorization, not a transferable allowance.

Use focused tests during implementation, then applicable multi-crate landing gates
and memory feature configurations. Serialize main-crate tests that share process
state. Review composition budgets if tools or schemas change; do not freeze a
future tool count in this plan.

## Ordered slices and parallel ownership

6A blocks implementation. After its contracts are frozen, assign disjoint file
ownership before parallel work:

1. **6B domain owner:** matching/decision module and its focused fixtures; no backend
   or host edits. This owner consumes frozen shared types.
2. **6C persistence owner:** canonical types, backend contract, SQLite/in-memory
   mutations, migration and receipt parity tests. No classifier or host edits.
3. **6D recovery owner:** managed-service requests and the new decision-work adapter,
   after 6C admission is available. No formation re-extraction changes by default.
4. **6E integration owner:** feature confirmation/routing and read-side cache/index
   adapters, after 6B–6D interfaces stabilize. Shared-file changes go to their single
   assigned owner rather than parallel edits.
5. **6F evaluation owner:** new corpus/labels and evaluator fixtures can proceed in
   parallel after 6A; end-to-end measurement waits for 6E.

6B and 6C can proceed in parallel against frozen interfaces. 6D and 6E remain
ordered where they share host files. A coordinator owns interface changes and
integration gates. This is a future work split, not current delegation or branch
creation. Every task remains unchecked until its implementation and evidence land.
