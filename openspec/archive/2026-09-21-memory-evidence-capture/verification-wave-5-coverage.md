# Wave 5C durable coverage contract

## Scope and compatibility

Branch: `feat/memory-wave5-coverage-contract`, starting at `3a79db14` (accepted
pre-eviction checkpoint work). Fixture: `tests/fixtures/formation.json` from that
revision, with synthetic version-2 coverage variants in `tests/formation.rs`.
Reviewed implementation: `f7842ab9`.
Reviewed lint-only gate repair: `54377a33`.

`omegon-memory` owns the new `FormationCoverage` type, validation, and immutable
completion contract. The host explicitly constructs its existing snapshots with
unknown coverage. Formation envelope version 2 requires coverage policy 1 and a
nonzero inclusive range ending at the available source frontier. Version 1 keeps
its existing wire representation and cannot declare coverage.

No SQL schema migration is required. The existing formation JSON column retains
the metadata. Older validators reject envelope version 2 instead of accepting it
after dropping coverage. Use a compatible reader for a store containing version-2
envelopes; do not downgrade by deleting metadata. Public Rust struct literals need
the new optional field. Existing host capture receipt payloads remain unchanged.

This slice establishes a storage contract, not a host incremental cursor. Range
declarations do not independently prove canonical source completeness or authenticity.
The producer must establish those properties before persistence. Empty scanned ranges
of non-evidence records are representable, but cannot complete extraction.

## Red and green evidence

The first run exposed a test-fixture omission (`StoreEpisode.affected_nodes`) as
well as unsupported version-2 formations. After fixing the fixture, all four new
tests failed against the old implementation. In particular, a legacy formation
silently discarded a coverage declaration and passed validation. The corrected
red run is `sh_0c1bf95cb001pW3gRqTkm9hgZQ.out`.

The four focused tests then passed (`sh_0c1c0082d001EOoXUrUOo7IJlU.out`):

| Scenario | Regression |
| --- | --- |
| Legacy snapshot remains unknown | `coverage_contract_validates_ranges_and_preserves_legacy_unknowns` |
| No silent coverage loss on legacy declarations | `coverage_contract_does_not_silently_discard_legacy_declaration` |
| Invalid coverage range | `coverage_contract_validates_ranges_and_preserves_legacy_unknowns` |
| Completion changes coverage | `coverage_contract_completion_is_immutable_across_backends` |
| Persistence, receipt replay, reopen, transport | `coverage_contract_survives_replay_reopen_and_transport` |

Additional boundary assertions cover a single-record inclusive range, an empty
non-evidence range, and unavailable sources with empty evidence. The existing
receipt-failure rollback test now uses a version-2 covered formation, preserving
its episode/index/vector/receipt rollback checks.

## Same-executor review

Review is same-executor, not independent. Examined JSON compatibility, source and
evidence bounds, backend parity, completion import, receipt replay, and downgrade
attempts. The validator cannot infer omitted eligible events from sequence gaps:
non-evidence canonical records also produce gaps. Accordingly, coverage is an
explicit producer declaration, and legacy snapshot frontiers never become coverage.

Completion checks both envelope version and coverage, preventing a valid version-1
payload from removing coverage during a pending-to-terminal transition. Existing
checks still protect source identity, evidence, truncation, and extractor model.
The host's two constructors use `None`, preserving its existing source-key hashing
and serialized payloads. Future version-2 host capture must include coverage in its
new policy-specific identity and verify source continuity before resuming a range.

## Landing gates

Toolchain: Rust 1.95.0 (`59807616e`), host `aarch64-apple-darwin`, repository Nix
environment. The memory matrix and reverse-dependent gate passed.

Passed gate sequence (`sh_0c1c0af1f0010sCq3TCmMZJRgr.out`):

- `cargo test -p omegon-memory --locked --test formation`
- `just test-crate omegon-memory`
- `cargo test -p omegon-memory --all-features --locked`
- `cargo test -p omegon-memory --no-default-features --locked`
- `RUST_TEST_THREADS=1 just test-commit`
- `just clippy-changed`

The implementation was committed while the reverse-dependent gate was running.
Clippy also ran separately as `just clippy-changed --base 3a79db14`, so its scope
includes the committed code rather than only the remaining documentation diff
(`sh_0c1c49a3300182jpnCIfTq4usJ.out`). The explicit-base invocation is the Clippy
acceptance gate.

The first explicit-base Clippy run failed on four pre-existing expressions in
`bus.rs`, `runtime_composition.rs`, `contribution_loading.rs`, and
`tui/agent_events.rs`. Those files had no diff against `3a79db14` when the errors
were diagnosed. Rust 1.95 requests three `is_none_or` simplifications and a guarded
`AgentEnd` match arm. Applied these as a separate lint-only change. The guarded
arm's false case reaches the existing no-op wildcard, preserving event behavior.
The corrected Clippy run passed for both affected crates
(`sh_0c1c7578c001zQtct6Gj6K6xG1.out`). Focused regressions passed on the lint-fixed
source with `OMEGON_NERD_FONT=1 RUST_TEST_THREADS=1 cargo test -p omegon --locked`
and separate filters `bus::tests` (83 tests), `runtime_composition::tests` (7),
and `tui::tests` (447), including authoritative idle recovery and second submission
without AgentEnd (`sh_0c1c9aff5001WZYabLlzE8sXn0.out`). The attempted
`contribution_loading::tests` filter matched no tests. Its caller coverage passed
with `extension_cli::tests` (19 tests, `sh_0c1cb38c8001PbxyWSHipg6Hlz.out`).

Full-gate results: 5,331 main unit tests passed (10 ignored), plus default integration
suites. Memory default/all-features runs passed 100 unit tests (one generator ignored)
and 71 integration tests. No-default-features passed 96 unit tests (one ignored)
and 64 integration tests. The opt-in PTY test remained ignored; live-provider smoke
wrappers did not establish live provider execution. Changed Rust files passed
`rustfmt --edition 2024 --check`.

Named OpenSpec validation and `git diff --check` passed before recording these
results. Runtime/provider execution is not evidence for this domain-only slice.

Transcripts are under
`/Users/wilson/.local/share/opencode/shell/9153e6c974f56bff7f08cdf3cb1bb0749fa67fbe/`.

## Handoff

G0–G4 pass for this scoped domain contract on 2026-09-20. Same-executor review has
no unresolved blocking finding. The coverage-contract slice is accepted.
Wave 5 and this parent corpus remain implementing. Next: bounded host range capture,
policy-specific stable page identities, durable watermark discovery scoped to
mind/session/stream/policy, and restart/overflow tests that prove no eligible event
is skipped. Queue/readiness contracts and comparative evaluation also remain open.
