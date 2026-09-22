+++
title = "Native Headroom — Next Session Plan"
tags = ["headroom","benchmarks","next-session"]
+++

# Native Headroom — Next Session Plan

## Preservation status and revival prerequisites

`workstream/native-headroom` is deliberately unre-based archived WIP, not a production-ready integration. It retains its original base `196ec5266847882f655ebe55e6e163c0dca2eb6b`; canonical main at preservation was `7b2da402073d84e54f3caffbb13845a007161c62`. The attempted rebase was aborted.

Before revisiting this work:

- Rebase onto then-current main and resolve integration debt before claiming readiness.
- Integrate Headroom settings through the shared settings projection, including mutation routing and persistence semantics. Do not restore the obsolete inline settings renderer.
- Adapt search limits and telemetry to the managed codescan binding and contracts boundary. Do not restore the obsolete embedded codescan engine architecture.
- Run fresh focused tests, applicable broader integration gates, and the live A/B matrix below. Keep compression opt-in/default-off until new evidence supports changing that posture.

Preservation validation is limited to `git diff --check`; no tests or benchmarks were rerun. The results below are historical evidence, not validation against current main.

## Historical evidence

- Strict core pack passes with `96%` evaluated savings, `0` restored facts, and `103/103` understanding questions passing.
- First real sparse-critical JSON A/B passed functionally and showed provider-token reduction:
  - off: `1,607,319` total provider tokens
  - on: `478,038` total provider tokens
  - delta: `-1,129,281` tokens, about `70%` reduction
- The preserved A/B report fix in `scripts/benchmark_harness.py` distinguishes token reductions, increases, and unchanged totals.

## Next task shapes

Run live Omegon A/B evidence with `--headroom-ab-real` in this order:

1. **Log failure diagnosis**
   - Long failing build/test log.
   - Acceptance: answer names failing test/error line and relevant source path.

2. **Diff risk review**
   - Large patch/diff.
   - Acceptance: answer names changed files/functions and at least one seeded behavioral/security risk.

3. **Retrieval-required original**
   - Compressed output intentionally omits an original-only detail but exposes CCR retrieval.
   - Acceptance: agent retrieves the original and answers with the original-only detail.

4. **Markdown decision recall**
   - Long design doc.
   - Acceptance: answer cites a decision, rejected alternative, and constraint.

5. **Code API explain**
   - Large Rust source/module.
   - Acceptance: answer explains target function contract, edge case, and relevant test/call site.

6. **Debug from log to code**
   - Failure log plus source inspection.
   - Acceptance: answer connects symptom to source mechanism and, where fixture-safe, proposes or applies the minimal fix.

## Policy rationale

JSON already has positive real A/B evidence. Log and diff are in the current default auto-kind candidate set, so they need live evidence first. Markdown, code, and broad plain text show strong corpus savings, but should remain gated until live task evidence proves no reasoning degradation.

## Metrics to capture per A/B task

- off/on status and acceptance score
- off/on provider-token totals
- token delta and percentage
- headroom eval status
- evaluated savings percentage
- restored fact count
- retrieval behavior, especially whether CCR was used when required
