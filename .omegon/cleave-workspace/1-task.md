---
task_id: 1
label: aggregate-mode
siblings: [0:real-run]
---

# Task 1: aggregate-mode

## Root Directive

> Run a real example benchmark task end-to-end, inspect the generated artifacts and report output, then add a minimal directory-level aggregation/report mode for benchmark result files with tests and docs updates as needed.

## Mission

Add a minimal directory-level aggregation/report mode for benchmark result files, shaped by existing result artifact structure. Include tests and doc updates if the mode is added.

## Scope

- `scripts/benchmark_harness.py`
- `tests/test_benchmark_harness.py`
- `docs/token-efficiency-comparison-harness-v1.md`

**Depends on:** real-run

## Siblings

- **real-run**: Execute a real benchmark run from the example task, inspect the generated artifacts and report output, and capture concrete findings about output shape and any runtime gaps. Do not edit code except if required to unblock the run and only within scoped benchmark harness files.



## Testing Requirements

### Test Convention

Write tests using pytest in co-located test_*.py files


## Contract

1. Only work on files within your scope
2. Follow the Testing Requirements section above
3. If the task is too complex, set status to NEEDS_DECOMPOSITION

## Finalization (REQUIRED before completion)

You MUST complete these steps before finishing:

1. Run all guardrail checks listed above and fix failures
2. Commit your in-scope work with a clean git state when you are done
3. Commit with a clear message: `git commit -m "feat(<label>): <summary>"`
4. Verify clean state: `git status` should show nothing to commit

Do NOT edit `.cleave-prompt.md` or any task/result metadata files. Those are orchestrator-owned and may be ignored by git.
Return your completion summary in your normal final response instead of modifying the prompt file.

> ⚠️ Uncommitted work will be lost. The orchestrator merges from your branch's commits.

## Result

**Status:** PENDING

**Summary:**

**Artifacts:**

**Decisions Made:**

**Assumptions:**
