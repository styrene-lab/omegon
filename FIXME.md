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

---

## 4. Canonical dispatcher-switch control request for Auspex/Omegon parity

**Context:**
Auspex is now typed-first internally and no longer uses raw legacy JSON as its production command model. Prompt submit, cancel, and slash/control actions have canonical typed paths, but **dispatcher switch still lacks a canonical Omegon control request over IPC/web control surfaces**.

**Observed gap in Omegon rc.63:**
- `core/crates/omegon/src/control_runtime.rs` defines typed `ControlRequest` variants for model/thinking/context/auth/plugin/secrets/cleave/delegate actions, but nothing for dispatcher switching.
- `core/crates/omegon/src/ipc/connection.rs` routes canonical typed requests like `set_model`, `set_thinking`, `delegate_status`, etc., but no dispatcher-switch request.
- Auspex currently has to serialize dispatcher switch to websocket compatibility JSON at the edge.
- On IPC, Auspex now explicitly rejects dispatcher switch as unsupported and surfaces a visible operator-facing failure instead of silently dropping it.

**What needs to be implemented in Omegon:**

### A. Add a canonical typed control request
Add a new control-runtime request for dispatcher switching, something structurally equivalent to:
- `ControlRequest::SwitchDispatcher { request_id: String, profile: String, model: Option<String> }`

This request must be treated as a first-class control surface, not a legacy/raw JSON special case.

### B. Route it through canonical ingress surfaces
Wire the new request through:
- `core/crates/omegon/src/control_runtime.rs`
- `core/crates/omegon/src/ipc/connection.rs`
- any web/daemon control bridge that already forwards typed `ExecuteControl` requests
- control action classification (`core/crates/omegon/src/control_actions.rs`)

The request must receive:
- a clear canonical action classification
- an explicit control role
- an explicit `remote_safe` decision

### C. Update runtime/session state publication
When a dispatcher switch is requested and processed, the runtime must continue publishing authoritative dispatcher state via the existing snapshot surfaces so Auspex can reconcile:
- `available_options`
- `switch_state`
- `expected_profile`
- `expected_model`

If there is already an internal runtime path that performs dispatcher switching, expose it through the new typed control request instead of duplicating logic.

### D. Keep request-aware reconciliation semantics
Auspex already expects monotonic request-aware reconciliation:
- pending local request gets a `request_id`
- snapshot-confirmed matching request becomes active/confirmed
- conflicting request IDs become “active elsewhere”
- backend rejection becomes failed with `failure_code` / `note`

Do **not** regress that. The new canonical request must preserve those semantics in published state.

**Acceptance criteria:**
- A canonical typed dispatcher-switch request exists in Omegon control runtime.
- IPC can execute dispatcher switch without using `run_slash_command` or raw JSON envelopes.
- Web/daemon typed control can execute the same request through the same runtime path.
- `control_actions` classifies the new request explicitly.
- Snapshot/state publication continues to expose authoritative dispatcher `switch_state` and target profile/model.
- Tests cover:
  - accepted dispatcher-switch request
  - rejected dispatcher-switch request
  - state publication after issue/confirm/fail
  - IPC routing for the new request
  - control classification for the new request

**Why this matters:**
This is the last meaningful non-canonical control seam for Auspex integration. Until Omegon exposes this request, Auspex must keep a compatibility-only websocket edge path and report IPC dispatcher-switch attempts as unsupported.

**Validation once implemented:**
From Auspex side, the expected follow-up is:
- remove the special unsupported IPC branch for dispatcher switch
- dispatch typed dispatcher-switch requests over IPC normally
- keep websocket compatibility only if still needed for remote degraded mode
- verify `cargo test`, `cargo clippy --all-targets -- -D warnings`, and an actual Auspex live attach/switch flow
