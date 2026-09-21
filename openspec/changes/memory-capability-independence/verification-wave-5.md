# Wave 5 capability verification

Branch: `feat/memory-wave5-completion`. **Accepted on 2026-09-21.** The parent wired
automatic indexing, recall, extraction observations, and memory-query readiness.
The final workspace gates, full memory feature matrix, optional native checks,
and independent review passed. See [joint acceptance](../../memory-wave-5-verification.md).
Integration-stage notes below retain the development history.

## Implemented slice

- `surfaces/memory_status.rs` projects storage, extraction, embeddings, keyword
  retrieval, semantic retrieval, and pending indexing from supplied observations.
- `status.rs` caches component observations per project root and exposes them in
  `HarnessStatus.memory_capabilities`. Reads preserve independent observations
  during a storage outage. Unknown pending counts are not converted to zero.
- `setup.rs` publishes configuration and disable reasons. A configured extractor
  does not claim successful inference. Skipped embedding discovery with no store
  is unknown, not a provider outage.
- `embedding.rs` owns the shared cancellation/deadline boundary for identified
  generation. CLI repair uses this boundary and retains its existing compatible
  space, version-check, and ready-vector skipping behavior.
- `local_embedding.rs` propagates dropped requests to its blocking worker with
  a cancellation token and ONNX Runtime's supported termination API.
- Schema v14 adds durable version/attempt/space-bound indexing records on both
  backends. Completion atomically writes compatible vector state, clears pending
  state, and records the receipt. Managed status exposes aggregate typed reasons.
- `embedding::index_fact` persists pending state before inference and settles
  cancellation with an independent bounded token. CLI repair now uses durable
  attempts and preserves identical ready vector data while clearing pending state.

## Scenario mapping

| Scenario | Test |
|---|---|
| Embeddings-only and unavailable-storage matrix | `wave5_capability_matrix_keeps_optional_components_independent` |
| Extraction outage with repair pending | `wave5_outage_and_pending_repair_are_independent_read_only_observations` |
| Configuration is not proof of readiness | `wave5_configuration_and_unknown_index_do_not_claim_semantic_readiness` |
| Repeated read-only status, root isolation, storage outage | `wave5_cached_readiness_is_read_only_and_survives_storage_outage` |
| Bounded embedding generation | `wave5_embedding_deadline_drops_owned_inference` |
| Cancellation during inference | `wave5_embedding_cancellation_drops_owned_inference` |
| Cancellation before inference | `wave5_precancelled_embedding_never_starts_inference` |
| Optional local worker cancellation signal | `wave5_local_request_drop_signals_the_blocking_worker` |
| Managed cancellation after commit and reopen | `wave5_cancelled_indexing_persists_reason_after_commit_and_reopen` |
| Both-backend late callbacks, incompatible spaces, immutable facts | `indexing_attempts_preserve_facts_reject_late_callbacks_and_repair_atomically` |
| Version drift and newer-attempt preservation | `indexing_old_versions_cannot_complete_or_overwrite_new_attempts` |
| Pending/timeout/repair persistence | `indexing_pending_timeout_and_completed_repair_survive_reopen` |
| Atomic rollback of vector, pending deletion, and receipt | `indexing_completion_rolls_back_vector_and_receipt_when_pending_cleanup_fails` |
| Schema migration | `schema13_migration_adds_indexing_without_changing_facts` |
| Real repair, no reinforcement, and ready-vector preservation | Extended `memory_repair_blackbox::backfill_repairs_legacy_vectors_and_skips_ready_facts_without_reinforcement`; a SQLite trigger rejects ready-vector rewrites |

## Checks

- Initial `cargo test -p omegon --bin omegon wave5_ --locked`: four readiness
  tests passed before the shared generation helper was added.
- The first helper integration build found an old `EmbedError` pattern in the
  CLI repair error branch. It was updated to the typed bounded error. This was
  a compile error, not behavioral RED evidence.
- Corrected `cargo test -p omegon --bin omegon wave5_ --locked`: seven tests passed.
- `cargo test -p omegon --test memory_repair_blackbox --locked`: passed the real
  CLI repair/reopen/no-reinforcement test, including the new pending timeout and
  ready-but-cancelled cases and the vector-rewrite rejection trigger.
- `cargo test -p omegon --bin omegon --features local-embeddings wave5_ --locked`:
  passed all nine tests after the parent resolved concurrent `binding_id`
  initializer errors. This includes local cancellation, the complete matrix,
  and managed cancellation/reopen.
- `cargo test -p omegon-memory --test capture_cursor --test indexing_attempts --locked`:
  passed all five capture tests and all five indexing tests.
- `cargo test -p omegon-memory --no-default-features --test indexing_attempts --locked`:
  passed all five indexing tests without the agent feature.
- `cargo test -p omegon-memory schema_contract_generate --locked -- --ignored`:
  regenerated the canonical schema contract for v14.
- `cargo test -p omegon-memory --lib schema_contract_is_current --locked`: passed.
- `cargo test -p omegon-memory --lib migration --locked`: four migration tests
  passed, including stale-plan rejection and post-commit backup preservation.
- `openspec.py validate memory-capability-independence`: passed, implementing.

No behavioral RED run is claimed for the new indexing API. Tests assert its boundary
and matrix contracts. Wave 3/4 evidence remains authoritative for existing
extraction isolation, identified repair, no-agent support, and no reinforcement.

## Remaining integration

`integration-wave-5.md` documents the completed durable APIs and parent-owned host
hooks. Automatic generation must call `index_fact`, query generation must use the
shared bounded helper, and host operations must publish typed component results.
The parent owns the combined final landing gates and archival decision.

## Additional cursor regression requested by the parent

`capture_cursor_uses_semantic_key_fields_after_receipt_reformatting` failed against
the original serialized-key comparison: the lookup returned None after key order
and whitespace changed. SQLite now compares mind/session/stream/policy fields and
uses `IS` for nullable model identity. The regression passed for both model states
and rejects differences in each key field. Lookup remains local-receipt-only and
still decodes the typed effect. The complete capture test target also passed.

Local tool transcripts identify the reproducible runs:

- Optional host slice: `sh_0c54fa29f0015y0OlfBhbsdK0Y`.
- No-agent indexing slice: `sh_0c54fe8d7001h7hIRVkvwoSfVm`.
- Cursor behavioral RED: `sh_0c55541b5001x7wfWHjhAkQKpQ`.
- Cursor/indexing GREEN: `sh_0c5564849001Spagxa71L3uEsO`.
- Enhanced real repair command: `sh_0c556580a0015lsYHpiT5ct8rt`.
- Schema contract check: `sh_0c55a16e8001iBYEcZWZ0JMtl8`.
