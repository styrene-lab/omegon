# FIXME — incomplete work, hand this to another agent

## 1. FYDS: make transient upstream retries cancel-bounded, not attempt-bounded

**File:** `core/crates/omegon/src/loop.rs`

**Function:** `stream_with_retry` (line 625)

**Current broken behaviour:**
- Retries transient errors up to `config.max_retries` times then gives up
- Shows "attempt X/Y; N left" in TUI — noisy and wrong
- Callers hardcode `max_retries: 3, retry_delay_ms: 2000` — too few, too slow

**What must change in `stream_with_retry`:**
- Remove the `attempt > config.max_retries` check for transient errors entirely
- Remove the "upstream provider exhausted after N retries" error path from the retry loop
- Transient errors must loop forever — the only exit is `Ok(msg)` or a non-transient error
- Change TUI notification messages from:
  `"⚠ Upstream stream stalled — retrying in {}ms (attempt {attempt}/{}; {} left): {short_err}"`
  to:
  `"⚠ Upstream stalled — retrying (attempt {attempt}, delay {}ms): {short_err}"`
  (no max, no "left")
- Change backoff cap from 10_000ms to 15_000ms
- Change initial delay from `config.retry_delay_ms` (which callers set to 2000) to 750ms hardcoded (or keep using config but fix callers)

**What must change in callers** (`core/crates/omegon/src/main.rs`):
- Line 1672: `max_retries: cli.max_retries` + `retry_delay_ms: 2000` — change `retry_delay_ms` to 750
- Line 1737: same
- Line 1902: same
- The `--max-retries` CLI arg (default 3) is now meaningless for transient errors; leave it in place but don't use it to cap transient retries

**`LoopConfig` struct** (line 19 in loop.rs):
- Keep `max_retries` field (don't break callers) but document that it no longer gates transient retry loops
- Update the comment from "Max retries on transient LLM errors" to "Unused for transient errors (those retry indefinitely until cancelled). Reserved for future non-transient caps."

**Test to update** (`core/crates/omegon/src/loop.rs` bottom):
- `loop_config_default_uses_aggressive_bounded_retry` — rename to `loop_config_default_retry_params` and update assertions to match new delay values

---

## 2. Cleave smoke harness: finish and run it

**File:** `core/crates/omegon/src/cleave_smoke.rs` — written but NOT yet executed or verified

**`--smoke-cleave` flag** wired in `main.rs` — calls `cleave_smoke::run(&cli)`

**What it does:** spins up a disposable git repo, runs the cleave orchestrator with injected child env vars, asserts status summaries and merge-result lines.

**Child injection hook** in `main.rs` function `maybe_run_injected_cleave_smoke_child`:
- `OMEGON_CLEAVE_SMOKE_CHILD_MODE=upstream-exhausted` → exits 2
- `OMEGON_CLEAVE_SMOKE_CHILD_MODE=fail` → exits 1
- `OMEGON_CLEAVE_SMOKE_CHILD_MODE=success-noop` → exits 0, no writes
- `OMEGON_CLEAVE_SMOKE_CHILD_MODE=success-dirty` → exits 0, writes `OMEGON_CLEAVE_SMOKE_WRITE_FILE`

**To verify it works:**
```
cd core && cargo build -p omegon && ./target/debug/omegon --smoke-cleave --model anthropic:claude-sonnet-4-6
```

**Known issue with `completed_with_merge` scenario:** the child writes to `README.md` inside the worktree but the worktree is a git branch. The auto-commit salvage path in `orchestrator.rs` (`salvage_worktree_changes`) must pick up the write and commit it before merge. This may or may not work — needs to be verified by actually running the smoke.

---

## 3. `injected_env` in orchestrator needs to be cloned before fallback dispatch too

**File:** `core/crates/omegon/src/cleave/orchestrator.rs`

Search for the fallback dispatch block (around line 430). It builds a `fb_dispatch: ChildDispatchConfig` — verify `injected_env: &config.injected_env` compiles correctly there (it was patched but the clone-before-spawn fix only covered the main spawn path, not the fallback retry path).

---

## Summary of state

- `cargo build -p omegon` currently passes (no compile errors)
- `cargo test -p omegon cleave_` passes (9 tests)
- `--smoke-cleave` is wired but has never been run
- FYDS changes have NOT been written — that is the primary outstanding task
