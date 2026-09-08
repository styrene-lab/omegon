# Wave 4 verification — identified retrieval and repair

## Scope and implementation

Branch: `fix/memory-retrieval-wave4`, based on the accepted adversarial hardening.
Implementation/test revision: `fa8b0959`.
The retrieval corpus owns this wave; capability independence receives the explicit
index-repair slice. Durable job scheduling and full component-status projection
remain separate work.

- Schema v10 adds nullable space metadata and a raw-content SHA256 to fact vectors.
  Supported v5–v9 migrations retain unknown identity on legacy rows.
- Identified mutations are version-checked, active-fact-only, and atomic with receipts.
  Legacy writes remain unknown; unidentified native queries fail explicitly.
- Identified search skips legacy, incompatible, and stale vectors before comparison.
  It retains bounded top-k results and reports typed diagnostics. Corrupt identified
  vectors and operational errors remain errors.
- Ollama uses documented model content digests checked around generation. Local ONNX
  loads the exact bounded artifact buffers that are hashed. No unverified identity
  is inferred from dimensions or a model label.
- Scores name lexical, cosine, fusion, and graph proximity. Relations preserve their
  original name and direction; conflicts do not boost existing seeds or reinforce facts.
- Backfill honors `--cwd`, uses bounded pages/deadlines and per-run operation identities,
  skips ready rows, rejects stale versions, and reports incomplete work as failure.

## Scenario-to-test mapping

| Requirement | Evidence |
|---|---|
| Historical population and low-confidence evidence | Existing initial-wave archive tests on both backends |
| Filters before limits | Updated `filtered_channels_respect_section_scope_and_read_only_history` with identified vectors |
| Explicit embedding spaces | `legacy_vectors_are_not_compared_to_an_identified_query`; `identified_spaces_repair_legacy_and_reject_model_revision_pipeline_or_dimension_drift` |
| Source freshness and optimistic writes | `changed_raw_content_requires_repair_and_old_versions_cannot_overwrite` |
| Migration, reopen, corruption, rollback | `identified_index_survives_reopen_and_corruption_is_an_error`; `schema9_migration_preserves_unknown_identity_and_failed_repair_is_atomic` |
| Named scores | `lexical_scores_are_named_and_conflicts_are_exposed`; `score_labels_preserve_fused_channels_and_passive_relations`; host `wave4_recall_reports_incompatible_vectors_and_named_scores` |
| Relationship meaning and history | `graph_conflicts_do_not_boost_seeds_and_history_preserves_status`; `supersession_direction_is_current_only_and_unknown_relations_do_not_expand` |
| Cancellation and numeric stability | `cancelled_empty_graph_is_not_a_successful_empty_result`; shared identified scan cancellation; `vector_math_is_finite_for_large_finite_values` |
| Host model identity | `wave4_ollama_identity_is_checked_around_generation`; optional `wave4_artifact_identity_covers_model_and_tokenizer_bytes` |
| Real repair command | `memory_repair_blackbox::backfill_repairs_legacy_vectors_and_skips_ready_facts_without_reinforcement` |

The initial two domain tests failed against the earlier behavior: same dimensions
were treated as compatible and lexical scores lacked a channel label. The CLI test
then exposed a real directory-selection bug: backfill ignored `--cwd`. That behavior
was fixed rather than changing the fixture's working directory to hide it.

Existing vector tests were migrated to the identified API so they still exercise
similarity and cooperative scans. Legacy API tests now assert identity-required
errors. Compile errors during adapter migration are not counted as behavioral red.

## Adversarial review

Same-executor review; no independent reviewer is claimed.

- Same-dimensional models, revisions, preprocessing changes, and dimension changes
  are all incompatible unless the complete identity agrees.
- A raw case-only content change invalidates a vector even when the normalized
  fact-deduplication hash is unchanged.
- Corrupt vector length and inconsistent stored model metadata are rejected rather
  than producing NaN scores, panics, or false readiness.
- Arithmetic uses wider accumulation so large finite f32 values remain finite.
- Source versions and receipts prevent stale or partially committed repair writes.
- A new repair run can restore derived state without incorrectly replaying an old
  completed write; ready records are skipped without changing facts.
- Supersession direction suppresses obsolete current expansion while preserving
  historical traversal. Passive relation names remain visible in rendered labels.
- Empty graph requests honor cancellation. Modern graph storage failures propagate.
- Local in-memory model loading avoids untracked external-weight dependencies.
  Artifact hashing/shape tests do not claim successful inference with a real ONNX model.
- Digest checks assume an honest stable Ollama server during a request. They do not
  establish cryptographic execution attestation or detect every possible ABA mutation.

## Final gate results

| Check | Result |
|---|---|
| `just test-crate omegon-memory` | Passed: 93 unit tests and 32 integration tests; one generator ignored |
| `cargo test -p omegon-memory --all-features --locked` | Passed |
| `cargo test -p omegon-memory --no-default-features --locked` | Passed |
| `RUST_TEST_THREADS=1 just test-commit` | Passed: 5,295 main-crate unit tests, default integration suites including the real repair CLI, and memory tests |
| `cargo test -p omegon --bin omegon memory_campaign --locked -- --ignored --test-threads=1` | All five portable campaigns passed |
| `cargo test -p omegon --bin omegon --features local-embeddings wave4 --locked` | Three optional-feature identity/adapter tests passed |
| `just clippy-changed` | Passed, all affected targets, including format check |
| `cargo clippy -p omegon --bin omegon --features local-embeddings --locked -- -D warnings` | Passed after correcting the local helper lint |
| OpenSpec validation and staged whitespace checks | Passed |

G0–G4 passed at `fa8b0959`. The adversarial pass was same-executor review; no
independent review is claimed. No blocking finding remains in this wave's scope.
The retrieval change is fully verified. The capability-independence corpus retains
its later status/scheduling work.

Local transcripts under
`~/.local/share/opencode/shell/9153e6c974f56bff7f08cdf3cb1bb0749fa67fbe/`:

- `sh_08252ad22001UAAVHjxWFyD1JP.out`: initial identity/score red.
- `sh_0829a1c7c001t7p5kgF5DAm1vA.out`: real CLI directory-selection red.
- `sh_082a1f12b0015BHhgOwghAjiMX.out`: corrected CLI repair green.
- `sh_082a1f2f5001r6GxKX696yDiTP.out`: optional local-embedding tests.
- `sh_082b06d4e001GSCrDmERy0GjVF.out`: complete crate feature matrix, serial affected-crate gate, and five campaigns.
- `sh_082b06dcf001y9GatZjXBjoHHF.out`: passing default Clippy followed by the optional helper lint.
- `sh_082bcd294001LhRy39Dg8E7qq3.out`: corrected optional Clippy gate.

## Compatibility and handoff

Schema migration is required before old stores can be opened by this backend.
Use the existing verified backup/recovery workflow; do not downgrade a binary over
a v10 store. New JSON score fields have deserialization defaults. Legacy hybrid
requests retain their response kind and use keyword fallback if identity is absent;
modern callers explicitly request diagnostics. No model-quality benchmark or ANN
performance claim is made.
