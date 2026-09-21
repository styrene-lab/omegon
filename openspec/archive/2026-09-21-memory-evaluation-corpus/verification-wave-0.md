# Wave 0 verification — initial offline fixture slice

The initial regression fixture and executable assertions live in
`core/crates/omegon-memory/tests/fixtures/retrieval.jsonl` and `tests/initial_waves.rs`.
The main-crate managed tool test reuses the same input fixture.

Fixture records contain no expected result lists or model answers. Assertions remain
evaluator-owned. Tests use isolated in-memory or temporary SQLite stores and no
network/model calls. Repeated historical search checks stable ID ordering and
unchanged fact transport. Distinct backends are checked for eligibility rather than
equal numeric relevance scores.

The first six tests produced behavioral failures for archive population, section
filtering, duplicate pins, and oversized-candidate starvation. Later coverage exposed
task-flood and FTS-error propagation defects. See the [Wave 1 evidence](../2026-09-21-memory-retrieval-contract/verification-wave-1.md)
and [Wave 2 evidence](../2026-09-21-memory-context-selection/verification-wave-2.md) for mappings.

This is the initial fixture slice, not completion of the evaluation corpus. Simulated
evidence cutoffs, future-event isolation, fake extraction availability, stage-level
reports, model experiments, and baseline comparisons remain pending in their tasks.

Same-executor adversarial review confirmed the observed red failures cannot pass by
reading expected IDs from the input fixture. G0–G4 passed for the initial supporting
fixture slice at `f6846622`; complete landing results are in the Wave 1 record.
The evaluation corpus remains implementing for its broader pending requirements.
