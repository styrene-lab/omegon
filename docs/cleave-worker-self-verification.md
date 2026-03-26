---
id: cleave-worker-self-verification
title: Cleave worker self-verification — PR-CoT repair loop before merge
status: exploring
tags: [architecture, cleave, verification, quality, self-repair]
open_questions:
  - "Should OpenSpec scenarios be the primary verification oracle (when present) with self-generated tests as fallback, or always both?"
  - "What is the right max repair round budget before declaring failure? ATLAS uses 3 — is that appropriate for Omegon's task surface?"
  - "Does self-verification add enough latency to warrant a config flag to disable it for fast/simple tasks?"
issue_type: feature
priority: 1
---

# Cleave worker self-verification — PR-CoT repair loop before merge

## Overview

Close the generate→verify→repair loop *within* each Cleave worker before it commits and
signals completion. Currently, Cleave workers generate code and commit. Quality review happens
post-merge via `/assess cleave`. This design moves verification earlier: each worker runs a
self-check cycle before declaring done, catching failures that would otherwise require a
post-merge remediation pass.

Inspired by ATLAS Phase 3 (PR-CoT multi-perspective repair): 85.7% rescue rate (36/42 failing
tasks recovered) with 7.3pp improvement on LiveCodeBench. The key insight: when all candidates
fail, interrogating the failure from four distinct perspectives yields repairs that a single
re-attempt would miss.

## Research

### PR-CoT four perspectives (from ATLAS)

```
logical_consistency      Check for logic errors: off-by-one, wrong conditionals,
                         incorrect operator usage

information_completeness Does the solution handle ALL cases in the spec? Missing
                         edge cases, unhandled input ranges, ignored constraints

biases                   Unstated assumptions: assumed input ordering, assumed
                         positive numbers, assumed connected graphs

alternative_solutions    Is there a fundamentally different algorithmic approach?
                         Different data structures, traversal orders, shortcuts
```

For Omegon's broader task surface (not just coding), a fifth perspective is warranted:

```
spec_alignment           Does this satisfy the OpenSpec Given/When/Then scenarios
                         for this change? (when OpenSpec change exists)
```

### Verification oracle hierarchy

ATLAS uses self-generated I/O test cases as the verification signal. This has a known failure
mode: if the model misunderstands the problem, it generates tests consistent with its
misunderstanding, repairs to pass those wrong tests, and incorrectly declares success.

Omegon has a stronger oracle available: OpenSpec Given/When/Then scenarios were written
*before* the code, not derived from it. They cannot be corrupted by the model's
misunderstanding of the implementation.

Proposed oracle priority:
1. OpenSpec scenarios for this change (when `openspec_manage` finds a matching change)
2. Existing test suite (`cargo test`, `npm test`, etc.) — ground truth, but expensive to run
3. Self-generated test cases — fast, fallback when neither above is available

### Placement in Cleave worker lifecycle

```
Current lifecycle:
  plan → assign files → generate → commit → signal done

Proposed lifecycle:
  plan → assign files → generate → self-verify → [repair loop] → commit → signal done
                                        ↑                               ↓
                                   (max 3 rounds)←──────── repair ←── fail
```

The self-verify step:
1. Generate acceptance tests (from OpenSpec, existing suite, or self-generated)
2. Run tests against the worker's output
3. On failure: run PR-CoT analysis from all perspectives
4. Generate repair candidates per perspective
5. Test each repair candidate
6. First passing candidate becomes the commit
7. After max rounds with no pass: commit with failure annotation for `/assess cleave` to handle

### Cost model

Each repair round = 2 additional LLM calls per perspective (analysis + repair). At 4
perspectives and max 3 rounds, worst case is 24 additional calls per worker. For a 3-worker
Cleave run, that's up to 72 additional calls only if all workers fail all tests on every round.

In practice: most workers pass on first verification or first repair round. ATLAS observed
~85% rescue on first round. The cost is bounded and justified — a failed Cleave that requires
a post-merge remediation pass costs more in total (human review + re-run) than the repair
budget spent inside the worker.

Disable path: a `verify: false` flag in Cleave plan or a `/cleave --no-verify` flag for
operators who want raw speed and will handle quality post-merge.

## Open Questions

- Should OpenSpec scenarios be the primary verification oracle (when present) with
  self-generated tests as fallback, or always run both in parallel?
- What is the right max repair round budget? ATLAS uses 3. Omegon's tasks are broader
  than LCB coding problems — may need tuning.
- Does self-verification add enough latency to warrant a per-task disable flag?

## Relations

- Modifies: Cleave worker lifecycle (`cleave-rs-daemon-port-execution-plan`)
- Uses: OpenSpec (`openspec_manage`) as primary verification oracle
- Inspired by: ATLAS PR-CoT (itigges22/ATLAS `benchmark/v3/pr_cot.py`, arxiv:2601.07780)
- Improves: `/assess cleave` (post-merge review becomes remediation for genuine edge cases
  rather than catching basic failures)
