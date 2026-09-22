+++
title = "Native Headroom — Next Session Plan"
tags = ["headroom","benchmarks","next-session"]
+++

# Native Headroom — Next Session Plan

## Current evidence

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
