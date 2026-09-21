# Wave 5 provenance closure audit

**Accepted on 2026-09-21.** Final combined gates passed; see
[joint acceptance](../../memory-wave-5-verification.md). All parent tasks are complete.

## Evidence boundary

Audited on 2026-09-21 in the shared `feat/memory-wave5-completion` worktree.
This record consolidates accepted slice evidence and verifies the current domain
tests. It does not substitute for the parent's final combined landing gates.
Earlier verification records remain historical records of their named revisions.
Their statements that later slices remain open describe that earlier point in time.

The delta specifications are the acceptance authority. Design documents explain
the implementation. Test bodies and current source were checked against each
scenario; a checked task is not itself behavioral evidence.

## Scenario-to-evidence closure

Paths below are relative to `core/crates/`. Domain test names refer to
`omegon-memory/tests/` unless a host path is named.

| Scenario | Executable evidence | Disposition |
|---|---|---|
| Assistant claim lacks verification | Host `features/memory/formation.rs::wave3_capture_retains_goal_correction_and_attributed_outcome`; `formation.rs::candidates_reject_fabricated_refs_authority_and_malformed_siblings` | Observed tool outcomes remain distinct from assistant reports; generated candidates cannot elevate authority |
| Retained episode evidence survives source-independent transport | `formation_evidence_and_pending_candidates_survive_transport`, `formation_evidence_survives_reopen`, `formation_completion_is_atomic_replayable_searchable_and_transportable` | Source-independent evidence retained; no active fact admission |
| Imported content cannot elevate its authority | `vault_authority.rs::imported_note_cannot_self_declare_operator_or_execution_authority` | New direct regression passes on both backends; forged frontmatter/prose cannot set source attribution or confirmation |
| Platform-specific workaround | `applicability.rs::platform_inapplicable_facts_cannot_consume_candidate_limit` | Linux mismatch excluded before candidate limit |
| Correction recorded after historical period | `validity_bounds_are_timezone_aware_and_history_retains_recorded_time` | Valid-time filtering retains recorded time and historical lifecycle labels |
| Revision change retires previously injected guidance | Accepted hosted applicability tests in `verification-wave-5-applicability.md`; `standalone_scoped_store_and_update_retire_live_context` | Fresh host HEAD and empty replacement retire guidance without deleting facts |
| Applicability updates do not reinforce claims | `scope_updates_are_versioned_without_reinforcement_and_survive_transport` | Versioned update preserves confidence/status/reinforcement |
| Scoped transport cannot masquerade as unrestricted legacy fact | Same scope-update test; `distinct_scopes_do_not_collapse_and_scope_replay_reuses_the_right_fact` | Distinct tag and legacy omission preservation |
| Legacy fact survives migration and transport | `schema8_migration_preserves_legacy_unknowns_and_rolls_back_on_failure`; `candidate_survives_reopen_and_schema10_migration_preserves_legacy_unknowns`; `schema11_pending_records_migrate_without_fabricating_confirmation`; transport tests; vault tests below | Legacy unknowns preserved; vault projections are not a lossless record codec |
| Historical records retain operational state in transport | `transport_history.rs::transport_preserves_history_and_operational_state_across_backends`; `history_edges_and_operational_state_survive_reopen_and_failed_export_is_not_partial` | Four established lifecycle populations, operational metadata, and edge endpoints preserved |
| Legacy update cannot reset known operational state | `legacy_updates_preserve_known_operational_state_and_modern_updates_replace_it` | Older/equal updates ignored; omitted operational fields preserved |
| Migration fails before completion | `schema8_migration_preserves_legacy_unknowns_and_rolls_back_on_failure`; `schema12_migration_and_failed_scope_write_preserve_records` | Failed migration/write preserves previous schema or records |
| Evidence source becomes unavailable | Host `inspection_reports_unavailable_declared_source_without_mutation`; `inspection_tracks_artifact_availability_without_reactivating_history` | Unavailable reported without deletion or reinforcement |
| Changed artifact snapshot remains inspectable | Host `inspection_tracks_artifact_availability_without_reactivating_history` | Changed digest reported without durable mutation |
| Readable declared reference remains unverified | Host `inspection_does_not_validate_declared_references_or_follow_unsupported_paths` | Readability cannot upgrade inference |
| Decision retains artifact identity | Host `lifecycle_artifact_validation_distinguishes_conclusions_from_chatter`; `lifecycle_conclusions.rs::receipt_failure_rolls_back_correction_and_reopen_preserves_attribution`; `explicit_corrections_are_atomic_versioned_and_portable` | Decisions, source hashes, reopen and JSONL preserved |
| Open question remains outside durable conclusions | Host `lifecycle_artifact_validation_distinguishes_conclusions_from_chatter` | Explicit constraint accepted; open question rejected |
| Archived specification retains its source | Host `lifecycle_artifact_scope_allows_baseline_and_archive_specs`; domain portable conclusion tests | Baseline/archive references accepted; active proposals rejected |
| Inferred lifecycle conclusion remains pending | Host `lifecycle_inference_is_pending_and_not_recalled_as_knowledge`; `lifecycle_candidates.rs::inference_replay_preserves_provenance_without_admitting_or_reinforcing_knowledge` | Pending, excluded from context/index/vault, no implicit correction |
| Confirmed correction atomically supersedes | `candidate_confirmation.rs::reviewed_candidate_confirmation_is_version_checked_and_durable`; `explicit_corrections_are_atomic_versioned_and_portable` | Atomic supersession and replay |
| Conflicting version prevents partial supersession | `changed_snapshot_and_correction_versions_reject_without_partial_admission`; `explicit_corrections_are_atomic_versioned_and_portable`; receipt-failure tests | No replacement, edge, or success receipt on conflict |
| Model-supplied approval cannot confirm | Accepted public-tool and finalized-bus tests in `verification-wave-5-confirmation.md` | Approval flags and non-runtime internal dispatch rejected |
| Operator confirms reviewed snapshot | `reviewed_candidate_confirmation_is_version_checked_and_durable`; `changed_snapshot_and_correction_versions_reject_without_partial_admission`; accepted TUI/ACP tests | Snapshot and target versions checked; inference attribution retained |
| Operator denial/cancellation preserves pending state | Accepted permission-bridge tests in `verification-wave-5-confirmation.md` | No mutation dispatched on denial/cancellation |

## Evidence retention and ownership findings

The design now states the implemented retention contract explicitly. Episode
snapshots retain bounded excerpts and host identities in durable formation metadata.
Lifecycle conclusions retain statement text, references, and hashes, not full
artifact bytes. Source disappearance changes inspection availability, not facts.
Transport preserves evidence independently of source accessibility.

The domain owns typed attribution, applicability, validation, and atomic mutation.
`inspection.rs` creates one status-neutral projection. Host `lifecycle.rs` owns
bounded repository-relative artifact access and uses the existing opsx parsers.
The renderer formats the projection; it does not decide authority. Public review
uses the existing permission channel and internal runtime mutation route.

Vault publication remains a projection. `vault_sync.rs` generates `memory_section`
pages, while import accepts `memory_fact` notes. Tests for unchanged import across
reopen, duplicate IDs, note identity relocation, traversal, and symlink boundaries
remain in that module. The new authority test also verifies unchanged reimport
after materialization does not alter any serialized fact field. It passes on the
existing implementation; no behavioral RED result is claimed for that new test.

The general RED tasks aggregate earlier accepted behavioral failures: unsupported
explicit admission, active inferred ingestion, absent source inspection, and
platform-limit displacement. The accepted slice records preserve their actual
red/green history. Migration/transport fixtures and the new import-authority test
are additional regression evidence, not invented historical failures.

## Validation on this worktree

- `cargo test -p omegon-memory --test vault_authority --locked`: passed, one test
  exercising both backends.
- The same `vault_authority` target with `--no-default-features --locked`: passed.
- `cargo test -p omegon-memory --locked --test formation --test transport_history
  --test lifecycle_candidates --test lifecycle_conclusions --test candidate_confirmation
  --test provenance_inspection --test applicability --test token_selection
  --test initial_waves --test vault_authority`: passed, 61 tests.
- `cargo test -p omegon-memory --lib vault_sync --locked`: passed, 28 tests,
  including reopen idempotency, unchanged generated projections, path traversal,
  symlink rejection, and note identity/lineage cases.
- Host scenarios above inherit the accepted slice runs. The parent must rerun the
  applicable main-crate and changed-crate gates after concurrent runtime edits.
- Named OpenSpec validation passed (`implementing`); scoped `git diff --check`
  passed. The remaining unchecked gate is intentional.

## Archive readiness

The provenance scenario coverage and retention decision are complete at the domain
boundary. The parent landing-gate task stays open until the combined gate passes
and this change validates. Do not treat this audit as permission to archive early.
Canonical event-range capture is owned by `memory-evidence-capture`; its runtime
cursor/coverage changes require separate completion evidence. No unverified
runtime capture guarantee is inferred from the retention tests here.
