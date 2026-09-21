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

## Wave 5D implementation boundaries

`omegon-memory/tests/support/evaluation.rs` owns shared evaluator support. The
main crate imports that test-only module for the opt-in provider experiment.
Fixture evidence and labels use separate files. `Case::view` compares parsed
timestamps and exposes only evidence at or before the simulated query instant.
Adapter and prompt constructors cannot receive a `Gold` value.

The offline extractor copies supported observations into controlled candidate
outputs. The real domain candidate parser validates source references. Candidates
become isolated, active fixture facts through the real JSONL import boundary.
This is controlled evaluation setup, not production candidate admission. Wave 6
still owns automatic admission and reconciliation.

The current-memory adapter uses SQLite or in-memory lexical retrieval and the
shipped shared selector at 1,024 accounted bytes. The candidate-policy adapter
ablates the cap to 256. The curated-file baseline filters unverified assistant
reports, searches source lines, and packs whole lines. Its curation labor is
unmeasured. All adapters disclose their different budgets.

The case cutoff is an injected applicability clock. Current decay APIs still
read wall time, so fixture fact timestamps use a fixed far-future epoch to
neutralize confidence decay. Original source timestamps remain in evidence views.
These experiments test evidence availability and selection, not simulated decay.
Existing decay and identified-vector tests own those independent contracts.

The live runner uses active `Profile` resolution and the existing execution route
service. It reserves UTF-8 prompt bytes, 4,096 overhead tokens, and the provider
output allowance before each request. One aggregate ledger covers development,
held-out work, and failures. Missing usage stops further dispatch and retains the
reservation. Existing-output refusal prevents accidental reset of the same run.
The Codex subscription bridge does not accept a requested output-token limit.
The subscription tier instead reserves the configured model's full registered
output ceiling, including reasoning, before dispatch. It corroborates the
configured grade-D model against the existing Codex subscription model catalog.
The native request has no unsupported output-limit fields. A short response-byte
guard and request timeout stop local consumption; unavailable usage retains the
full reservation and stops further dispatch. Models whose full allowance cannot
fit the remaining aggregate budget cannot dispatch. Existing OAuth resolution
refreshes the stored subscription grant or adopts usable Codex CLI credentials.
The subscription tier forbids a serving-model change or paid-API fallback.
No alternative credentials or endpoints are supplied.

Continuation reports preserve prior attempt accounting. The initial unavailable
route can release its reservation because resolution failed before `route.stream`.
Its elapsed time remains charged. Other failed or unknown-dispatch attempts retain
their reservations. Configuration is written before development inference, and
the measured development summaries and thresholds are frozen before held-out views.

Development-selected smoke thresholds are persisted before live held-out views
are constructed. The split is a small authored synthetic corpus, not a blinded
external benchmark. A completed smoke comparison cannot establish general model
quality. Unavailable runs cannot satisfy acceptance.

Reader answers use a constrained `claims` JSON array of verbatim source quotations,
or exact `ABSTAIN`. Evaluator-only scoring verifies required source IDs, presence
in selected context, and equality with cutoff-eligible source claims. Keyword
rubrics remain supplemental checks and cannot establish success alone. Arbitrary
prose is uncertain; unsupported or contradictory quotations miss. The rule applies
uniformly to both splits and all adapters without exposing gold requirements.
