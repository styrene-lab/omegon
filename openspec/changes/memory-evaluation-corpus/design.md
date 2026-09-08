# Evaluation design

## Ownership and fixtures

Domain fixtures belong in `core/crates/omegon-memory/tests/fixtures/`; multi-session
fixtures and integration runners belong in `core/crates/omegon/tests/fixtures/memory/`
and the main crate's test targets. Follow existing runner conventions before adding
a new harness. Keep OpenSpec scenarios as the behavioral contract, with stable
case IDs mapped to executable tests.

Each case separates timestamped inputs, workspace applicability, source IDs, fake
model outputs, expected eligible evidence IDs, and held-out assertions. Include:
corrected assumptions, platform workarounds, repeated failures, superseded
decisions, old valid constraints, unsupported assistant claims, abstention,
embedding outages, and task-irrelevant memory floods.

## Two verification tiers

The deterministic tier uses fake clocks, fixed vectors, fake extractors, and
scripted task outcomes. Assert exact statuses, eligible IDs, evidence links, and
budget boundaries. Rank assertions apply only where the contract fixes ordering.
Do not require SQLite BM25 and an in-memory lexical implementation to produce
identical numeric relevance values.

The model tier fixes reader/extractor identities, prompts, seeds where supported,
corpus revision, model configuration, and token/cost accounting. Preserve per-case
outputs and judge rubrics. Treat judge uncertainty and failed runs separately from
incorrect answers. Compare no memory, curated files plus search, current memory,
and the changed policy. Record ingestion cost as well as query cost.

Before held-out runs, freeze development-selected success thresholds, regression
tolerances, and latency budgets in the run manifest. Live evaluation requires
explicit execution intent and a declared budget. CI relies on deterministic
tests rather than external inference availability.
