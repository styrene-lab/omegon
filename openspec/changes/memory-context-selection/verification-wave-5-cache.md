# Wave 5 semantic selection cache

## Scope

Branch: `feat/memory-wave5-selection-cache`.
Selection owners reuse one bounded snapshot while request semantics, backend state,
and eligibility remain valid. Schema remains v13; cache identity and epochs are
process-local, not persisted fact vocabulary.

The cache retains ranking for at most 30 seconds. It expires before known validity
or confidence-floor transitions and on clock reversal. It does not claim that
every fractional decay-score change reranks the snapshot immediately.

## Tests

The hosted unchanged-turn test initially reported no cache hit. Core tests use a
controlled cache clock and a counted retrieval closure to verify actual suppression
of retrieval calls. They cover future facts absent from the original selection,
archive updates, task/target/pin/policy/budget changes, wall-clock expiry and reversal,
external SQLite commits, backend instance changes, confidence-floor boundaries,
and writes during computation.

Native timing scans read metadata on misses. Hits do not rescan the fact store.
Custom backends or renderers without reliable cache contracts remain uncached.

## Verification

Passed:

- `just test-crate omegon-memory`: 99 unit and 66 integration tests.
- `cargo test -p omegon-memory --all-features --locked`: 99 unit and 66 integration tests.
- `cargo test -p omegon-memory --no-default-features --locked`: 95 unit and 59 integration tests.
- Final focused `--lib selection_cache` run: all seven cache tests, including the
  subsequently added exclusive-validity-end regression and HEAD-key assertion.
- Hosted unchanged-turn reuse, standalone/hosted adapter parity, and explicit
  `request_context` pack parity tests.
- All five ignored portable `memory_campaign` tests, run separately after the
  broad gate stopped.
- `just clippy-changed` for both affected crates.
- Named OpenSpec validation and whitespace checks.

The full feature-matrix runs preceded the final test-only additions. The existing
schema generator remained ignored. No schema or dependency migration was needed.

The initial `RUST_TEST_THREADS=1 just test-commit` did not pass. The macOS linker exited with
status 69 because the Xcode license had not been accepted. This prevented linking
main-crate integration targets and the binary. After the operator accepted the
license, `RUST_TEST_THREADS=1 just test-commit --base f16cddfb^` passed on
2026-09-16 in the existing environment. The explicit base includes the committed
cache changes; the bare command skipped testing because it found no affected crates.

The completed gate passed 5,318 main-crate unit tests (10 ignored), the default
main-crate integration suites, and 100 memory-crate unit tests (one ignored) plus
66 memory integration tests. Opt-in live-provider tests are not evidence of live
provider execution. The environment blocker is resolved and this cache slice is
accepted. The parent corpus remains implementing for its remaining work.

Execution records are under
`/Users/wilson/.local/share/opencode/shell/9153e6c974f56bff7f08cdf3cb1bb0749fa67fbe/`:
`sh_0a5f635c00019Du7JJY0WobsbS.out` records the hosted RED result;
`sh_0a60cdfc9001lQNvjGxWz9bM14.out` records focused GREEN results;
`sh_0a60f6df7001WjKv3z53Fe2N51.out` records the passing crate matrix and Xcode blocker.
`sh_0aaec5a03001wmVHKJhV2td1fH.out` records the successful final gate after license acceptance.

## Same-executor adversarial review

This was a same-executor review, not an independent review. Reviewed risks include
excluded future facts, exclusive validity ends, local and external mutations,
backend replacement, clock movement, confidence-floor transitions, budget changes,
and writes during retrieval. Tests cover these boundaries and counted retrieval
suppression. Adapter parity checks exclude only snapshot-specific metadata.

The review added explicit coverage for exclusive `valid_until`, HEAD identity, and
the remaining report flags. It also documented counter identity as a cache contract.
No blocking implementation finding remains from this review. The final gate now
passes as recorded above.

This slice provides deterministic reuse evidence, not a live-model quality,
latency, or cost benchmark. Comparative evaluation and checkpoint recovery remain
separate pending work.
