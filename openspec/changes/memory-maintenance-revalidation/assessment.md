# Post-Wave-5 drift assessment — Wave 7

## Scope and authority

This assessment reconciles the planned maintenance change against source and test
definitions at the post-Wave-5 baseline identified by the parent as `7b2da402`.
Inspection occurred in the shared `feat/memory-wave6-reconciliation` worktree.
The parent owns branch isolation and upstream issue creation. No Wave 6 acceptance
or Wave 7 implementation is claimed by these documentation edits.

Source and executable tests establish current behavior. Accepted baseline contracts
establish obligations. This change's proposal, design, delta spec, and unchecked
tasks describe planned behavior, not evidence of implementation. Tests were read
for this audit, not executed. Named OpenSpec validation checks document structure.

## Findings ordered by implementation consequence

All paths below are relative to the repository root.

| Classification | Finding and evidence | Reconciliation |
|---|---|---|
| Current scaffold; planned extension | `core/crates/omegon-memory/src/maintenance.rs`: DormancyPlan sorts deterministic dry-run candidates; apply uses ID-only dormancy_facts. Its test expects first apply 1, second 0. | Extend this owner with FactPrecondition and payload-bound receipts. Do not claim original-outcome replay exists already. |
| Current reusable contract | `core/crates/omegon-memory/src/backend.rs`: apply_mutation, mutation_receipt, apply_mutation_bound; `types.rs`: FactPrecondition and TransitionFacts. | Freeze whole-plan write-set and reuse receipt-first admission. Add source preconditions and atomic work/evidence completion through the same owner. |
| Stale task framing | Original tasks implied retrieval/rendering/transport needed new no-reinforcement fixes. `tests/retrieval_wave4.rs`, `tests/provenance_inspection.rs`, `tests/transport_history.rs`, `tests/applicability.rs`, and `tests/indexing_attempts.rs` already assert related invariants. | Characterize and preserve existing behavior; seek genuine RED only for remaining gaps. |
| Current coupled policy | `core/crates/omegon-memory/src/decay.rs`: effective_confidence multiplies persisted confidence by reinforcement-dependent age decay; ambient_score excludes below 0.10. `maintenance.rs` uses the same floor. `selection_cache.rs` derives deadlines with confidence_floor_deadline. | Change all four paths together for old supported knowledge. Evidence support and attention need policy before migration. |
| Current remaining writes | `core/crates/omegon-memory/src/vault_sync.rs`: related_facts drives ReinforceFactOnce and facts_reinforced reporting. `backend.rs` documents duplicate-store reinforcement; `inmemory.rs` and `sqlite.rs` retain store/reinforcement paths. Host `memory_service.rs` carries reinforce_references. Root AGENTS.md describes decay reset. | Freeze report/config compatibility and change write-side attention under Wave 6 source-independence decisions. Later implementation must reconcile operator guidance. |
| Current validity; planned query extension | `core/crates/omegon-memory/src/applicability.rs` validates scope and inclusive-from/exclusive-until time. `types.rs` SearchFilter selects only Archived/Dormant/Superseded for Historical. | Preserve archive search. Add explicit all-status retained history with as-of assessment so Active-but-expired facts remain discoverable without archiving. |
| Current local lookup is insufficient | `core/crates/omegon-memory/src/backend.rs` has mind-scoped status-neutral get_fact_record. This is exact-ID inspection, not all-status history search. | Do not present that lookup as satisfying the discoverability contract. Apply new query semantics before limits across adapters. |
| Current inspection/indexing; planned revalidation | `core/crates/omegon-memory/src/indexing.rs` checks attempt identity, fact version, and embedding space. Host `core/crates/omegon/src/features/memory/lifecycle.rs` reads secure source snapshots and validates explicit conclusions. | Source readability, unchanged hash, and index freshness do not imply claim revalidation. Add separate check/verification attribution and work contract. |
| Current bounds; planned scheduler | `vault_sync.rs` bounds files and bytes and checks cancellation. Host lifecycle snapshot reads at most 1 MiB on supported Unix hosts. `memory_service.rs` caps fact pages at 1,000 and owns managed requests. | Preserve these boundaries; explicitly choose scheduler cadence, fanout, fairness, capacity, backoff, deadline, and recovery limits. Cancellation is cooperative, not a kernel-I/O guarantee. |
| Current transport; incomplete old plan | `core/crates/omegon-memory/src/types.rs`: JsonlFact and FactOperationalState transport confidence, reinforcement, last_accessed, lifecycle times, and other history. `tests/transport_history.rs` checks modern/legacy semantics. | Use a field-by-field matrix. Keep existing historical fields portable while new local attention and queue/receipt/progress state remain local. |
| Current schema; planned migration | `core/crates/omegon-memory/src/sqlite.rs`: MEMORY_SCHEMA_VERSION is 14. | Use 14 only as the observed baseline. Allocate the next migration from integrated state after policy freeze. |
| Unknown dependency acceptance | The root memory modernization map labels Waves 6–8 planned. A sibling reconciliation branch is not an accepted integrated contract. | Record Wave 6 contract/commit/verification references before correction integration; allow DTO-frozen pure work independently. |
| Planned quality evidence | Existing evaluation fixtures neutralize wall-clock decay for earlier policy tests (`tests/support/evaluation.rs`). Earlier Wave 5 results do not test this new kind-aware aging policy. | Build a fresh stale-versus-old-valid corpus with negative cases, development-frozen thresholds, held-out labels, and fresh authorization for live spending. |

## Decisions and gates still required

- 7A owns policy and DTO freeze: classification, review conditions, confidence
  interpretation, checked-versus-verified semantics, and report/config compatibility.
- One shared-types/schema owner freezes the atomic whole-plan write-set and exact
  wire representation. Other owners consume that contract without competing edits.
- 7B specifies concrete new scheduler limits and retry classification. Existing
  reader limits do not by themselves bound aggregate scheduling work.
- Wave 6 acceptance is pending. Its source-independence and correction decisions
  must be consumed, not independently recreated by Wave 7.
- Wave 8 needs the accepted evidence-versus-attention/validity slice reference;
  unrelated scheduling completion is not its blanket prerequisite.
- 7F freezes numerical development thresholds before held-out use. No current
  quality score or live spending authorization is inferred from Wave 5.

## Update sequence and verification boundary

The proposal now identifies the remaining scope and dependency gates. The design
defines ordered groups 7A–7F, write-set semantics, source uncertainty, query
compatibility, and the transport matrix. Delta scenarios make these contracts
observable. Tasks retain unchecked implementation and acceptance work throughout.

Validation for this planning-only adjustment is named-change OpenSpec validation,
whitespace checking, and review of the scoped diff. Behavioral and runtime gates
remain implementation tasks. Upstream branch and issue handling remains with the
parent session.

The named validator returned `memory-maintenance-revalidation: OK (planned)`.
The scoped `git diff --check` passed. Implementation tests were not run for this
documentation-only adjustment; all implementation tasks remain unchecked.
