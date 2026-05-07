+++
id = "317f53b4-f64d-44cc-8b59-85b916957fc2"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Assessment: cleave-child-timeout

**Result: PASS**
**Date: 2026-03-18**

## Scenario Results

| # | Scenario | Result | Evidence |
|---|----------|--------|----------|
| 1 | Child stalls with no activity | ✅ PASS | Rust: `tokio::time::timeout` on stderr reader (orchestrator.rs:330) fires after idle window. TS: `resetIdleTimer()` + `setTimeout` fires after `idleTimeoutMs` (dispatcher.ts:734-745). Both kill immediately. |
| 2 | Active child keeps resetting idle timer | ✅ PASS | Rust: each stderr line resets `last_activity` (orchestrator.rs:332). TS: `resetIdleTimer()` called on every RPC event (dispatcher.ts:769). |
| 3 | Wall-clock timeout fires at 15 minutes | ✅ PASS | `DEFAULT_CHILD_TIMEOUT_MS = 15 * 60 * 1000` (dispatcher.ts:48). Rust: `tokio::time::sleep(wall_timeout)` in `tokio::select!` (orchestrator.rs:319). |
| 4 | Custom idle timeout | ✅ PASS | `idle_timeout_ms` param in cleave_run schema (index.ts:2279) → `idleTimeoutSecs` in NativeDispatchConfig → `--idle-timeout` CLI arg → Rust `idle_timeout_secs`. |

## Design Decision Compliance

| Decision | Status | Verified |
|----------|--------|----------|
| Activity-gap idle timeout + reduced wall-clock | decided | ✅ Both Rust and TS paths implement |
| Kill idle children immediately | decided | ✅ Rust: kill_on_drop + explicit kill. TS: killCleaveProc |
| Dual-backend idle detection | decided | ✅ Rust stderr primary, TS RPC resume fallback |

## Test Coverage

- `dispatcher.test.ts`: 59 tests pass, including dedicated timeout constant tests (IDLE_TIMEOUT_MS=3min, relationship to wall-clock, custom parameter acceptance)
