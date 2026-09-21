# Wave 5C bounded startup recovery

## Scope and compatibility

Branch: `feat/memory-wave5-recovery`, based on accepted selection/cache work at
`d4448105`. This slice adds startup consumption of existing pending formation
envelopes. It uses schema v13 and formation wire version 1 without migration.

The domain owns a pending inventory filtered by mind and exact recorded model
before the eight-record limit. The host owns startup scheduling, extraction,
deadlines, and shutdown. Completion reuses the atomic source/model validation,
episode search update, vector invalidation, and receipt contract.

The startup pass runs once per feature instance, with a 120-second total budget.
It preserves remaining pending work for later startup. Continuous scheduling,
interval/pre-eviction capture, retry/backoff state, full readiness projection, and
candidate-admission batch recovery remain open. Provider execution can repeat after
interruption or concurrent discovery; committed completion cannot overwrite a
terminal result or manufacture an active fact.

## Red and green evidence

`cargo test -p omegon --bin omegon startup_recovery_completes --locked` initially
failed with extractor calls `0`, expected `1`. The implementation then passed the
expanded `startup_recovery` test filter:

- `startup_recovery_completes_durable_pending_evidence_once`: closes and reopens
  the SQLite managed owner, creates fresh features, and verifies one extraction,
  one retained episode, terminal completion, and unchanged source/evidence.
- `startup_recovery_shutdown_cancels_extraction_and_retains_pending`: waits on an
  explicit extractor notification, invokes managed shutdown, verifies future drop,
  and reopens the database to confirm pending evidence remains.

`cargo test -p omegon-memory --test formation recovery_inventory --locked` passes:

- Filters completed records and other models/minds before the bound.
- Returns the same deterministic oldest-first inventory through both backends.
- Completes eight records and retains the overflow across SQLite reopen.
- Rejects an oversized limit and reports malformed SQLite evidence as an error.

## Same-executor review

This is a same-executor review, not independent review. Reviewed startup admission,
model changes, disabled/child extraction policy, bounded inventory, cancellation,
terminal-state exclusion, concurrent completion, and retained evidence identity.
The host follows its existing extractor configuration; an absent extractor starts
no recovery work. A different model does not select the old model's checkpoints.
The domain transition rejects a second completion after a concurrent winner.
Recovery errors stop the pass, retaining remaining work, and diagnostics omit
provider output and evidence content.

No blocking implementation finding has been identified in this scoped review.
The broader queue and observability contracts remain future slices rather than
claims of this startup pass.

## Landing gates

All scoped landing gates passed:

- `just test-crate omegon-memory`
- `cargo test -p omegon-memory --all-features --locked`
- `cargo test -p omegon-memory --no-default-features --locked`
- `RUST_TEST_THREADS=1 just test-commit`: 5,320 main-crate unit tests passed
  (10 ignored), default integration suites passed, and memory tests passed
  (100 unit plus 67 integration; one schema generator ignored).
- All five portable `memory_campaign` tests, explicitly run with `--ignored`.
- `just clippy-changed` for both affected crates.
- Named OpenSpec validation for evidence capture, context selection, and provenance;
  `git diff --check`.

The opt-in live-provider suite is not evidence of live provider calls. Existing
compact-unwind and dependency future-incompatibility warnings remain.
Execution transcript: `sh_0ac780418001xQZz1Un1QTUFPx.out` under
`/Users/wilson/.local/share/opencode/shell/9153e6c974f56bff7f08cdf3cb1bb0749fa67fbe/`.
The bounded startup-recovery slice is accepted. Wave 5C and the parent corpus remain
implementing for the remaining scheduling, checkpoint, and observability contracts.
