# Retrieval closure audit

**Accepted for archival on 2026-09-21.** The final workspace gates and memory
feature matrix passed; see [joint acceptance](../../memory-wave-5-verification.md).

Audited on 2026-09-21 in `feat/memory-wave5-completion`. Waves 1 and 4 already
accepted the retrieval contracts. This audit checks every delta scenario against
current test bodies and retains their original red/green records.

Test paths below are under `core/crates/omegon-memory/tests/` except where noted.

| Scenario | Evidence |
|---|---|
| Archived match is discoverable | `initial_waves.rs::tools::{sqlite,inmemory}_archive_population`; accepted live `initial_wave_live_tool_contracts` |
| Old low-salience evidence is searchable | Same archive-population tests; `filtered_channels_respect_section_scope_and_read_only_history` asserts no reinforcement |
| Section filter survives hybrid retrieval | `tools::{sqlite,inmemory}_section_before_limit`; `filtered_channels_respect_section_scope_and_read_only_history` |
| Equal dimensions from different models | `retrieval_wave4.rs::identified_spaces_repair_legacy_and_reject_model_revision_pipeline_or_dimension_drift`; accepted host `wave4_recall_reports_incompatible_vectors_and_named_scores` |
| Legacy vector needs repair | `legacy_vectors_are_not_compared_to_an_identified_query`; identified-space repair test; accepted real `omegon/tests/memory_repair_blackbox.rs` repair/replay test |
| One fact participates in both channels | `score_labels_preserve_fused_channels_and_passive_relations`; `lexical_scores_are_named_and_conflicts_are_exposed`; accepted host score test |
| Ineligible neighbor cannot bypass current filtering | `filtered_channels_respect_section_scope_and_read_only_history`; `graph_conflicts_do_not_boost_seeds_and_history_preserves_status`; `supersession_direction_is_current_only_and_unknown_relations_do_not_expand` |
| Contradiction is surfaced as conflict | `lexical_scores_are_named_and_conflicts_are_exposed`; `graph_conflicts_do_not_boost_seeds_and_history_preserves_status` |
| Quotes and operators are input data | `filtered_channels_respect_section_scope_and_read_only_history`; existing quoted-token backend tests accepted in Wave 1 |
| Episode search quoted names/title matches | `formation.rs::adversarial_episode_search_{sqlite,inmemory}` |
| Lexical storage fails during hybrid recall | `historical_search_survives_reopen_and_storage_failure_is_not_empty` injects a missing FTS table |

## Current checks and remaining integration

- The closure-domain command recorded in
  `../2026-09-21-memory-provenance-validity/verification-wave-5-closure.md` passed all 61 tests,
  including initial-wave and episode-search scenarios.
- `cargo test -p omegon-memory --test retrieval_wave4 --locked`: 11 passed.
- Named OpenSpec validation passed (`verifying`). The read-only
  `archive memory-retrieval-contract --check` command reported `ARCHIVE READY`.
- The accepted Wave 4 record supplies the real CLI repair, optional model-identity,
  full feature-matrix, and host adapter evidence. Those checks were not rerun by
  this audit executor.

No uncovered retrieval scenario or implementation gap was found. All existing
tasks are already checked. The structural archive check passes; the parent should
archive only after its combined runtime gates. Nothing was archived here.
