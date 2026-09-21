# Wave 5D evaluation evidence

**Accepted on 2026-09-21.** The Codex subscription continuation passed the frozen
smoke thresholds, and the parent completed the full workspace and memory feature
gates. Independent re-review resolved all evaluator findings. See
[joint acceptance](../../memory-wave-5-verification.md) and `codex-continuation.md`.
Earlier unavailable runs and integration-stage notes below are retained as history.

## Final result

The existing Codex subscription completed the paired development and held-out
experiment and passed the frozen smoke thresholds. Both roles used
`openai-codex:gpt-5.6-luna`. The cumulative total was 7,541 measured tokens and
149.616 seconds, including prior attempts. See `codex-continuation.md` and
`live-codex-subscription-wave5.json` for final results, exact configuration,
independent-review corrections, and credential cleanup. Initial unavailable
reports remain preserved. General model-quality superiority is not claimed.

## Implementation and evidence boundaries

Branch: `feat/memory-wave5-completion`. This evidence accompanies the shared
working-tree implementation. No separate subagent commit was created.

- Shared runner: `core/crates/omegon-memory/tests/support/evaluation.rs`.
- Domain tests: `core/crates/omegon-memory/tests/evaluation_corpus.rs`.
- Input-only cases: `tests/fixtures/evaluation.jsonl` in the domain crate.
- Evaluator labels: `tests/fixtures/evaluation-labels.json` in the domain crate.
- Host runner: `core/crates/omegon/src/memory_evaluation.rs`.
- Raw outputs: `offline-wave5.json`, `live-wave5.json`, and
  `live-wave5.json.manifest.json` in this change directory.

The runner tests real JSONL ingestion, candidate-reference validation, backend
lexical retrieval, and shared whole-block selection. Controlled candidate
admission and a scripted reader isolate policy behavior in the offline tier.
This does not claim live session capture or automatic active candidate admission.
Existing capture, provenance, vector, and host-adapter tests cover those owners.

## Scenario mappings

| Scenario | Executable evidence |
|---|---|
| Correction arrives after query | `cutoff_and_labels_never_reach_writers_or_readers`; future source IDs fail candidate validation |
| Parsed instants rather than lexicographic timestamps | `fixture_schema_rejects_labels_and_compares_actual_instants` |
| Repeated offline evaluation | `deterministic_real_backends_and_paired_budgets`; repeated full semantic rows on both backends |
| Correct evidence dropped during packing | `packing_miss_absence_and_failed_dependencies_are_separate`; formation/retrieval pass while selection/task miss |
| Missing evidence requires abstention | Same test, plus `unsupported_correct_guess_does_not_hide_missing_evidence` |
| Contradictory answers share expected keywords | `keyword_overlap_cannot_turn_contradictions_into_success`; also rejects corrupted writer modality with retained source IDs |
| Failed dependency and missing cost fields | Same stage-attribution test; unavailable extraction propagates unavailable stages and nullable costs |
| Comparative reports and unequal budgets | `deterministic_real_backends_and_paired_budgets`, `heldout_offline_report` |
| Oversized lexical distractor and policy ablation | `budget_ablation_preserves_small_evidence_after_oversized_lexical_noise` on both backends |
| Host consumes same evaluator | `memory_evaluation_offline_host_uses_shared_contract` |
| Aggregate budget cannot dispatch another call | `aggregate_budget_exhaustion_stops_before_dispatch` |

Initial tests failed because fixture import silently skipped an invalid decay
profile. Explicit import-count validation now prevents empty-store false greens.
The next run exposed real wall-clock decay in simulated-time retrieval. Fixed
fixture timestamps neutralize that dependency; this suite does not claim an
injected production decay clock. The one-byte packing case remains an intentional
negative control: retained and retrieved evidence is lost only at selection.

The oversized-noise ablation demonstrates lower injection size without losing
the small required evidence. The authored held-out split tests future correction,
unsupported assistant certainty, repeated-command failure, and an older valid
platform constraint. It supplements the historical/scope/flood fixtures from
Wave 0 and the richer counted-budget fixtures from Wave 5B.

## Frozen policy and initial unavailable attempt

The frozen manifest records corpus and label SHA-256 digests, exact model names,
prompt revision, default model settings, lexical-only retrieval, resource caps,
and development summaries. Seed and temperature overrides were not requested.
The extraction output ceiling is 16,384 tokens. The task call timeout is 45 seconds.

The resolved active profile selected reader `anthropic:claude-sonnet-4-6`.
Extraction used the host default `anthropic:claude-haiku-4-5-20251001` because
the resolved profile supplied no extraction override. A legacy project singleton
file named a different reader; active `Profile` resolution is authoritative.

The authorized aggregate limit was **100,000 input/output tokens and 600 seconds**.
There was one routing attempt, for development extraction. Existing routing
returned **no configured provider route** after 405 ms. The harness stopped
dispatch, retained a conservative **20,949-token reservation**, and recorded
**0 measured tokens**. The original report retained unknown costs. Later routing
review proved that resolution failed before inference dispatch, so the Codex
continuation released this reservation and retained the 512 ms elapsed time.
No provider usage or USD price was available in this initial report.

All 32 initial live arm/case rows are incomplete. The raw report records unavailable
formation and downstream stages independently. Zero selected-evidence recall
in those unavailable memory rows describes the empty trace, not a measured
quality failure. Task success and error-rate aggregates are null. Neither this
attempt nor its smoke-scale corpus establishes live model-quality acceptance.

The deterministic development arms achieved 4/4 supported task outcomes for
file search and both memory caps. No memory achieved 1/4 through the abstention
case. These measurements selected the following smoke thresholds before live
held-out construction: task success at least 0.75, evidence recall 1.0, zero
stale/repeated-error rubric matches, no candidate regression, and complete paired
rows. Those thresholds remain frozen despite unavailable live execution.

The final scorer uses constrained verbatim claims with selected-source and
cutoff-eligible source checks, not an LLM judge. Raw responses are retained for
inspection. Non-conforming prose produces a separate `uncertain` judgment, which
blocks acceptance. Fabricated or contradictory quotations miss even when expected
keywords and source IDs appear. Smoke results remain extractive evidence probes,
not general model-quality claims. See `codex-continuation.md` for independent
review corrections and the user-selected subscription continuation.

## Validation

Focused command results and parent landing gates are recorded below as they
complete. The parent owns combined shared-tree gates and joint archival.

- `cargo test -p omegon-memory --test evaluation_corpus --locked`: eight passed.
- The same target with `--no-default-features`: eight passed.
- `cargo clippy -p omegon-memory --test evaluation_corpus --locked -- -D warnings`: passed.
- The `memory_evaluation::` main-crate filter passed both offline/dispatch-budget
  tests. The live test remains ignored during ordinary CI.
- The explicit live test preserved initial unavailable reports, then completed
  the user-selected subscription comparison. Final live acceptance passed as
  recorded in `codex-continuation.md`.
- After parent integration, `token_selection` passed three host tests and
  `features::memory::tests::inspection` passed three host tests. Earlier compile
  blockers are resolved. Parent combined landing gates remain required.
- Named OpenSpec validation passed for evaluation and context selection.
- Rustfmt ran on all three evaluator Rust files. Whitespace checks passed.

The JSONL fixture is explicitly staged because the repository ignores new
`*.jsonl` files. Other evaluator changes remain unstaged. No commit or archive
was created, and no parent-owned implementation was modified by this agent.
