# Post-Wave 5 source assessment

## Boundary and authority

Assessed revision: `7b2da402` (`origin/main` when the seed branch
`feat/memory-wave6-reconciliation` was created). This is a source-based planning
assessment, not implementation or runtime verification. Source establishes current
behavior. Archived acceptance records establish the limits of prior evidence.
This change's delta specifies planned behavior, not existing guarantees.

## Material findings

| State | Finding and planning consequence | Evidence at assessed revision |
| --- | --- | --- |
| current | `MemoryCandidate` has only `content`, `section`, and `evidence_ids`, nested in `EpisodeFormation`. No individual admission ID or state exists there. Freeze those contracts before matching work. | `core/crates/omegon-memory/src/types.rs`, `MemoryCandidate`, `EpisodeFormation` |
| current | Lifecycle inference uses separate pending facts. Operator review binds the candidate snapshot and target version; confirmation currently directly activates the candidate. Do not confuse this path with formation candidates. | `core/crates/omegon/src/features/memory.rs`, `MEMORY_INGEST_LIFECYCLE`, `MEMORY_CONFIRM`, and `MEMORY_APPLY_CONFIRMATION` routing; `core/crates/omegon-memory/src/lifecycle.rs` |
| stale | Treating Wave 5 recovery as candidate admission recovery overstates it. Recovery resumes pending extraction through the extractor. Decision/admission recovery is new work. | `core/crates/omegon/src/features/memory/recovery.rs`; `core/crates/omegon/src/features/memory.rs`, startup recovery tests |
| stale | Atomic candidate-batch persistence is not per-candidate active admission. Completed formation persistence does not provide individual admission progress. | `core/crates/omegon-memory/src/types.rs`, `CompleteFormation`; `core/crates/omegon-memory/src/sqlite.rs` and `inmemory.rs`, formation completion handlers |
| current | Mutation receipts and entity-version preconditions already provide reuse points. Extend both backends through the managed service rather than introducing another persistence owner. | `core/crates/omegon-memory/src/types.rs`, `MemoryMutation`, `FactPrecondition`; `core/crates/omegon/src/memory_service.rs`, `ApplyMutation` |
| planned | Semantic matching, refinement/correction/conflict decisions, independent-support accounting, and recoverable admission remain Wave 6 work. Declared attribution is not proof of independent support. | This change's `specs/memory/reconciliation.md`; current candidate and mutation types above |
| unknown | Storage-key format, concrete decision-record owner, admission-unit grouping, refinement representation, and review-plan binding are unresolved. Close them in 6A. | This change's `design.md`, contract-freeze table |
| current | The inspected code uses SQLite schema 14; this does not assert that operator stores were migrated. Select the next migration when implementation starts. | `core/crates/omegon-memory/src/sqlite.rs`, `MEMORY_SCHEMA_VERSION` |
| stale | Wave 5 evaluation acceptance cannot establish semantic reconciliation quality. Its 32 comparison rows and four held-out cases are synthetic quotation probes. | `openspec/archive/2026-09-21-memory-evaluation-corpus/verification-wave-5.md`; `openspec/memory-wave-5-verification.md`, live comparison |
| planned | New development and held-out false-merge, duplicate, correction, authority, and task-regression evidence needs measured frozen thresholds and fresh live authorization. | This change's evaluation requirement and 6F tasks |
| current | Main-crate tests share process-global state and need serialization. New tools/schemas require measured composition-budget review. Preserve the CI lessons without projecting future counts. | `.github/workflows/test.yml`; `fixtures/composition-budgets-v1.json`; `docs/binary-composition-and-kernel-admission.md` |

## Evidence distinctions

Formation evidence event IDs locate source observations. They do not establish that
two observations are independent. Repeated extraction or paraphrase can share the
same underlying source. Admission operation IDs identify effects, not support.
The 6A evidence contract must distinguish declared provenance from validated
independence and deduplicate source support across different operation IDs.

`FormationCaptureKey` explicitly describes a local namespace that imports do not
advance (`core/crates/omegon-memory/src/types.rs`). Transported formation coverage
is source metadata, not a local cursor or successful local admission receipt.

Existing explicit writes do not need blanket approval. The preserved inferred
lifecycle policy requires operator confirmation. Extending that confirmation to a
reconciliation plan requires new binding rules, not claims that Wave 5 already
reviewed semantic decisions.

## Smallest coherent update sequence

1. Freeze 6A identity, ownership, admission-unit, refinement, and confirmation decisions.
2. Implement bounded matching and atomic admission against frozen shared contracts.
3. Add decision recovery, then host/read-side integration and stale-version checks.
4. Measure the new corpus and run landing gates under newly authorized live limits.

The historical 100,000-token/600-second Wave 5 budget does not authorize another
experiment. No tests or provider calls were used for this assessment. OpenSpec
validation and whitespace checks assess only these planning artifacts.
