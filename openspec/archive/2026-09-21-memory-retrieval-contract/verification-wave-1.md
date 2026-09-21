# Wave 1 verification — current/historical search contracts

## Scope and identity

Implementation branch: `fix/memory-initial-waves`.
Source starting revision: `cb68608c40bbaa99282f9057856e5c156fecf392`.
Planning artifacts were committed separately as `9017a423`.
Implementation candidate: `f6846622` (source and regression fixtures).

Scoped requirements: explicit historical statuses; historical recall without the
ambient floor; section/mind/status filtering before limits; malformed-query tolerance;
observable operational search failures. Embedding-space identity, explanatory scores,
and contradiction relation semantics remain Wave 4 work.

## Red and green evidence

The new test target `core/crates/omegon-memory/tests/initial_waves.rs` consumes only
synthetic `tests/fixtures/retrieval.jsonl`. Expected IDs remain in test assertions.
No user database, model calls, or credentials are used. Freshness timestamps are
fixed synthetic anchors; this is not yet a simulated as-of evaluation runner.

| Behavior | Executable evidence | Red result | Green result |
|---|---|---|---|
| Historical status population | tools::{sqlite,inmemory}_archive_population | Both returned active facts instead of archived/dormant/superseded IDs | Passed |
| Section before limit | tools::{sqlite,inmemory}_section_before_limit | Architecture distractors displaced the constraint at k=1 | Passed |
| Live tool adapter | features::memory::tests::initial_wave_live_tool_contracts | Archive tool returned active facts | Passed |
| Vector and graph filters, read-only repeat, quoted terms | filtered_channels_respect_section_scope_and_read_only_history | Added as adversarial regression coverage | Passed |
| Reopen and operational FTS errors | historical_search_survives_reopen_and_storage_failure_is_not_empty | Dropped FTS table was silently reported as no matches | Passed after preserving query-row errors |

Commands observed:

- `cargo test -p omegon-memory --test initial_waves --locked`: initial six tests
  failed for expected behavior; subsequent nine-test run passed after fixes.
- `cargo test -p omegon --bin omegon initial_wave_live_tool_contracts --locked`:
  behavioral red, then passed in the `initial_wave` run.
- `cargo test -p omegon --bin omegon memory --locked`: 103 passed, 5 intentionally
  ignored before the final compatibility/TTL regression additions.
- An initial `--lib` invocation was invalid because Omegon is a binary crate. It
  was corrected to `--bin omegon`; that invocation is not behavioral red evidence.

## Adversarial review

Reviewer: implementing executor, separate same-executor adversarial pass. No
independent review is claimed.

- Blocking finding: SQLite discarded iterator row errors, hiding a dropped FTS
  table as empty results. Fixed by collecting fallible rows before scoring. The
  injected-failure test now passes.
- Checked historical records remain read-only and exclude foreign/current records.
- Checked quoted query input cannot escape FTS quoting and matching terms survive.
- Checked section filtering applies before vector truncation and after edge loading.
- Checked new managed request fields have serde defaults; legacy requests retain
  current-search semantics and context requests default to no explicit query.

No DB schema change is required. Existing trait methods remain available; external
backends that do not implement filtering reject unsupported filters rather than
returning silently unfiltered data. Wave 4 owns richer vector compatibility.

## Gates and recovery

G0–G4 passed for the documented Wave 1 slice at `f6846622`. G3 was a disclosed
same-executor adversarial review. No unresolved blocking finding remains in this
scope. Revert code to restore previous policy;
there is no schema rollback or destructive data migration in this slice.

## Landing check recovery after harness restart

Host: macOS; `rustc 1.97.0 (2d8144b78 2026-07-07)`.

| Check | Observed result |
|---|---|
| `just test-crate omegon-memory` | Passed: 93 unit tests, 9 integration tests; one existing generator ignored |
| `cargo test -p omegon-memory --all-features --locked` | Passed: 93 unit tests, 9 integration tests; one existing generator ignored |
| `cargo test -p omegon-memory --no-default-features --locked` | Passed: 89 unit tests, 4 integration tests; one existing generator ignored |
| `cargo test -p omegon --bin omegon memory --locked` | Passed after restart: 104 tests, five existing opt-in campaigns ignored |
| `just clippy-changed` | Passed for omegon and omegon-memory, all targets, including format check |
| OpenSpec validation of the three participating changes | Passed as implementing |
| `git diff --cached --check` for implementation candidate | Passed |
| `RUST_TEST_THREADS=1 just test-commit` | Passed: 5,282 main-crate unit tests, default integration suites, and 93 memory unit plus 9 memory integration tests; existing opt-in/ignored tests remain excluded |

The original combined landing process completed all memory-crate configurations,
then was cancelled by a harness restart during the broad main-crate run. Its known
tiny-budget assertion still expected None; it was updated to require the empty
replacement that retires stale content. The corrected scoped runtime suite passes.
The native file-watch test that exceeded 60 seconds in that broad run passed in
isolation in 6.8 seconds. The serial broad gate subsequently passed, including that test.

Clippy identified collapsible conditions and a test-only cloned singleton slice.
Those were corrected and the full changed-crate lint command passed. The reviewed
candidate includes those corrections. No independent review is claimed.

## Handoff

The first shipping slice is accepted at `f6846622`. The archive/filter defects and
operational-error masking are fixed. Existing schema/reopen tests and the new
historical-query reopen fixture pass without a database migration.

The parent retrieval change remains implementing: embedding-space identity, typed
score explanations, full relationship semantics, and indexing repair belong to
Wave 4. The next scheduled implementation wave is independent extraction plus
evidence-backed formation, subject to its recorded model/default and minimal
provenance entry decisions. No corpus was archived.

## Local execution artifacts

Managed tool transcripts are retained under
`~/.local/share/opencode/shell/9153e6c974f56bff7f08cdf3cb1bb0749fa67fbe/`.
These are local execution artifacts; tests and synthetic inputs are committed so
another checkout can reproduce the checks without this directory.

| Evidence | Transcript file |
|---|---|
| Initial six behavioral failures | `sh_0813a1e1300132nsy1F4292U8r.out` |
| Live tool behavioral failure | `sh_0813ce637001fqFBpsVRMvgpET.out` |
| Task-context flood failure | `sh_0813fa5500017Rtl0dBkSAHZAw.out` |
| Empty-selection TTL failure | `sh_0814242e8001pMHohP6T7pJJ7N.out` |
| FTS operational error hidden as empty | `sh_0814243bd001GyJhYtTmVPrMLb.out` |
| Memory crate/default/all-features/no-default-features gates | `sh_081519167001ah3mr6v73B6DSt.out` |
| Resumed main-crate memory suite | `sh_0814efca3001li1JX9mgVREdPc.out` |
| Passing Clippy/format gate | `sh_081521d8b001yTiH1xO1NT6BG1.out` |
| Serial affected-crate gate | `sh_081521ced001mkLPPL9euXvCa1.out` |
