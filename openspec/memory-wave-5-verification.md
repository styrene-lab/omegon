# Wave 5 acceptance and baseline handoff

Wave 5 is accepted on 2026-09-21 on `feat/memory-wave5-completion`, based on
`4785970b`. The reviewed implementation is committed as `c922504b`.
The six foundational changes are ready for joint archival after their
final named validation and archive checks. Waves 6–8 remain planned.

## Accepted foundations

- Retrieval filtering, history, embedding identity, score semantics, and repair.
- Provenance, applicability, explicit confirmation, evidence retention, and transport.
- Shared token-counted context selection, inspection, and semantic cache invalidation.
- Independent component readiness and schema-v14 durable indexing attempts.
- Incremental evidence capture, local atomic cursors, bounded canonical replay,
  finalization queues, cancellation, and replay-safe candidate batches.
- Offline stage-attributed evaluation and a bounded live subscription comparison.

## Final gates

Environment: Rust 1.95.0 (`59807616e`), `aarch64-apple-darwin`, repository Nix tools.

Passed:

1. `just lint`: workspace format, check, and all-target Clippy with warnings denied.
2. `RUST_TEST_THREADS=1 just test-rust`: serialized full workspace tests and doctests.
   Main unit tests: **5,352 passed**, 11 intentionally ignored. Default integration
   suites also passed, including the real memory-repair CLI and protected ready vectors.
3. `cargo test -p omegon-memory --all-features --locked`.
4. `cargo test -p omegon-memory --no-default-features --locked`.
5. `cargo test -p omegon --locked --bin omegon --features local-embeddings wave5_`
   with serialized tests: **nine passed**.
6. Focused capture/cursor, bounded replay/blob, source-fencing, observation, evaluator,
   selection, inspection, and migration checks recorded in the participating changes.

The complete successful gate transcript is `sh_0c570d5ef0010zuYQ6AlAhaylU.out` under
`/Users/wilson/.local/share/opencode/shell/9153e6c974f56bff7f08cdf3cb1bb0749fa67fbe/`.
The broad workspace gate supersedes an additional affected-crate-only run.

The first full run failed one architecture guard because a test-only evaluator
file lacked the guard's recognized inline test-module boundary. The evaluator now
uses `#[cfg(test)] mod tests`; the ownership rule was preserved. Its focused test,
the evaluator tests, and the complete gate rerun passed. No production persistence
owner was moved outside the managed memory service.

## Live comparison

The operator authorized 100,000 aggregate tokens and 600 seconds, then selected
the existing OpenAI Codex subscription. Both frozen roles used the verified
`openai-codex:gpt-5.6-luna` model. The run used 40 successful calls:

- 5,569 input tokens plus 1,972 output tokens: **7,541 total**.
- **149.616 seconds** including the prior unavailable attempts.
- Every call reserved the registered 65,536-token output ceiling before dispatch.
- No incomplete or uncertain final rows, no retries, and no outstanding reservations.

| Held-out configuration | Supported task success | Required-evidence recall |
| --- | --- | --- |
| No memory | 1/4 | 0 |
| File search | 4/4 | 1 |
| Current memory | 4/4 | 1 |
| Candidate context policy | 4/4 | 1 |

All configurations had zero stale-memory and repeated-error rubric matches.
The result passes the frozen **synthetic smoke** thresholds. It does not establish
general model-quality superiority or superiority over file search. Unknown USD
pricing remains disclosed rather than fabricated. Raw reports and frozen manifests
are retained with `memory-evaluation-corpus`.

## Review and compatibility

Independent reviewers audited scenario coverage, capture/replay ownership, source
fences, evaluation budget admission, contradiction handling, and durable indexing.
All reported blockers were resolved and rechecked. Their review was source/artifact
inspection; test execution is attributed separately to the parent and implementers.

Schema v14 adds local indexing work and preserves legacy facts through tested
migration/backup paths. Cursor receipts remain local; imported episodes cannot
assert local capture progress. Version-1 formations retain unknown coverage.
Generated candidates remain unadmitted inferences until the later reconciliation
and admission contracts apply. Replay bounds are cooperative and do not claim to
interrupt a kernel filesystem syscall.

No installation or operator database migration was required for this acceptance.
Runtime evidence uses isolated managed-service fixtures, the real repair CLI, and
the explicit live provider runner. The existing untracked audit cursor is unrelated
to this change and is excluded from commits.
