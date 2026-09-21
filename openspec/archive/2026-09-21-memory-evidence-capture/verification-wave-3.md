# Wave 3 verification — evidence-backed formation

## Scope and decisions

Branch: `fix/memory-formation-wave3`, based on the accepted initial-wave branch.
Implementation and test revision: `f90e793e`.
The later [post-acceptance adversarial review](adversarial-review-wave-3.md)
reproduced additional defects and records their fixes. This original gate record
is retained as historical evidence, not a substitute for that follow-up verdict.
Participating changes: evidence capture, capability independence, and the minimal
episode-provenance slice. Full fact applicability, lifecycle-authority admission,
component status, interval checkpoints, durable scheduling, and automatic pending
work recovery remain open.

- Preserve the shipped Rust cheap extraction model, with independent profile model
  and enable/disable controls. Keep child inference disabled. Amend the conflicting
  historical model baselines explicitly in the delta rather than changing baseline
  files before archival.
- Read canonical semantic replay through the captured session-view binding. Use
  full-spine evidence and an explicitly limited suffix for mixed legacy sessions.
  Do not use advisory first/last message fragments as substitute evidence.
- Persist a typed optional formation envelope in schema v9. Source snapshots commit
  before inference; completion is a separate atomic mutation. Candidates remain
  unadmitted inferences. JSONL remains the typed transport.

## Red/green evidence

| Behavior | Test/evidence | Observed progression |
|---|---|---|
| Evidence survives transport and reopen | `omegon-memory/tests/formation.rs` initial two tests | Red: formation metadata deserialized away as null. Green after typed storage/migration. |
| Extraction configured without embeddings | `wave3_extraction_is_configured_without_embeddings` | Red with the original coupled builder, green after independent configuration. |
| Initial goal survives first-step boundary | `wave3_capture_retains_goal_correction_and_attributed_outcome` | Red after constructing a valid replay: initial prompt excluded. Fixed full-spine versus mixed-boundary handling. |
| Corrupt evidence remains observable | `formation_evidence_survives_reopen` | Red: corrupt metadata silently disappeared. Fixed fallible episode reads/export. |
| Classified pending candidates | `wave3_classifies_candidates_and_rejects_fabricated_references` | Controlled extractor retains Constraints and rejects unsupported references. |
| Candidate authority and bounds | `candidates_reject_fabricated_refs_authority_and_malformed_siblings` | Rejects invented source IDs, authority fields, bad siblings, oversized output, and invalid evidence-frontier/outcome combinations. |
| Source before inference | `wave3_cancellation_keeps_source_and_pending_extraction_durable` | Cancellation after extractor admission leaves source evidence and pending state durable. |
| Atomic completion and sync | `formation_completion_is_atomic_replayable_searchable_and_transportable` | Both backends reject changed evidence, replay completion, preserve zero active facts, and synchronize pending-to-complete metadata. |
| Compound rollback | `failed_completion_rolls_back_episode_index_vector_and_receipt` | Forced receipt failure rolls back the metadata/index/vector changes; successful completion invalidates the old vector. |
| Migration compatibility | `schema8_migration_preserves_legacy_unknowns_and_rolls_back_on_failure` | v8 backup/reopen succeeds; injected migration failure retains v8 without a partial formation column. Existing legacy migration loop covers v5–v8. |

Test-construction errors in semantic fixture request linkage and manifest ordering
were corrected against the authority owner's contracts. Those failures are not
counted as behavioral red evidence.

## Adversarial review

Same-executor review; no independent reviewer is claimed.

- Found that persisting only after extraction could lose evidence during shutdown.
  Source now commits first; cancellation is tested through the actual pipeline.
- Found initial prompts precede the first full-spine step boundary. Fully semantic
  sessions now include those prompts; mixed lineage remains explicitly partial.
- Found corrupt episode formation rows could be dropped during reads/export.
  Read errors now remain observable.
- Added thinking/restricted-content exclusion, missing-blob, generation-replacement,
  Unicode capture-bound, source-immutability, and receipt-failure cases.
- Added an episode FTS update trigger and atomic stale-vector invalidation because
  completed candidates can change searchable narrative content.

## Final validation

| Gate | Result |
|---|---|
| `just test-crate omegon-memory` | Passed: 93 unit tests plus 6 formation and 9 initial-wave integration tests |
| `cargo test -p omegon-memory --all-features --locked` | Passed: same unit/integration coverage |
| `cargo test -p omegon-memory --no-default-features --locked` | Passed: 89 unit tests plus 6 formation and 4 non-agent initial-wave tests |
| `cargo test -p omegon --bin omegon wave3 --locked` | Passed: 8 scoped host tests, including expanded content-boundary cases |
| `RUST_TEST_THREADS=1 just test-commit` | Passed: 5,285 main-crate unit tests, default integration suites, and memory tests; existing ignored/opt-in tests remain excluded |
| `just clippy-changed` | Passed for both affected crates, all targets, including formatting |
| `pkl eval --format json pkl/Profile.pkl` | Passed |
| OpenSpec validation of participating changes | Passed as implementing |
| Whitespace check of staged implementation | Passed |

The generated schema contract was regenerated through `schema_contract_generate`
and records schema v9. Optional formation data is boxed internally to keep existing
transport/request enums compact; this does not change JSON wire representation.

G0–G4 passed for this Wave 3 slice at `f90e793e`. G3 was same-executor adversarial
review, not independent review. No unresolved blocking finding remains in this scope.
Parent corpora remain implementing for their later-wave requirements; none is archived.

Local command transcripts reside under
`~/.local/share/opencode/shell/9153e6c974f56bff7f08cdf3cb1bb0749fa67fbe/`:

- `sh_08181eedd001McZGzghbWUhBJ4.out`: original formation round-trip/reopen red.
- `sh_0818303ea001tjTHSukc3mISAb.out`: original extraction/embedding coupling red.
- `sh_08194897e001ELnJNW7Vgl5tWk.out`: initial-goal boundary red after valid fixture construction.
- `sh_081971e9b001ye6xBuRr38So8h.out`: corrupt metadata disappearing red.
- `sh_081d73244001JnZ1HKE8LbZ7i9.out`: final expanded scoped host tests.
- `sh_081e7402e001czzgbnUPLGrK4S.out`: complete memory feature matrix and serial affected-crate gate.
- `sh_081e7402e002YxX72ojcSLRQ6f.out`: passing changed-crate Clippy/format check.

## Recovery and handoff constraints

This is a schema migration. Use the supported migration backup/recovery workflow;
do not run an older backend against v9 or restore over newer writes. Pending source
records survive interruption, but automatic restart scheduling remains Wave 5.
Completed operation replay does not rerun extraction. No live-model quality or cost
improvement is claimed from controlled fixture tests.
