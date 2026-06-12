---
title: Honker Research for Assistant Run Substrate
status: decided
tags: [architecture, assistant-runs, sqlite, honker]
---

# Honker Research for Assistant Run Substrate

## Summary

Honker is a strong future candidate for local assistant-run queueing, pub/sub wakeups, event streams, and scheduled assistant work. It is not required for the first assistant-run backend substrate. The current direction is to proceed with the existing `rusqlite`-based local ledger and keep queue/event abstraction seams so Honker can replace or augment the plain SQLite mechanics later.

Decision: **use plain SQLite now; keep Honker as a preferred future queue/pubsub/scheduler backend candidate.**

The main reason to defer adoption is dependency/build validation, not architecture. Honker fits the desired local messaging shape, but it is new, low-download, Rust-2024, and must be validated against Omegon's release/toolchain/dependency constraints before becoming a hard dependency.

## Upstream research sources

- `https://honker.dev`
- `https://honker.dev/docs/`
- `https://honker.dev/languages/rust/`
- `https://honker.dev/guides/queues/`
- `https://honker.dev/guides/pubsub/`
- `https://honker.dev/reference/bindings/`
- `https://honker.dev/reference/extension/`
- `https://honker.dev/roadmap/`
- `https://crates.io/api/v1/crates/honker`
- `https://crates.io/api/v1/crates/honker-extension`
- `https://raw.githubusercontent.com/russellromney/honker/main/Cargo.toml`
- `https://raw.githubusercontent.com/russellromney/honker/main/LICENSE`

## Crate facts

### `honker`

- Latest observed version: `0.3.4`
- Published: 2026-06-05
- License: `MIT OR Apache-2.0`
- Edition: `2024`
- Declared `rust_version`: none observed in crates.io metadata
- Downloads observed during research: low/new (`140` total)
- Repository from crates.io: `https://github.com/russellromney/honker-rs`
- Description: SQLite-native task runtime for durable queues, streams, pub/sub, and scheduler; ergonomic Rust wrapper over `honker-core`.

Features observed for `0.3.4`:

- `bundled-sqlite` → `honker-core/bundled-sqlite`, `rusqlite/bundled`
- `kernel-watcher` → `honker-core/kernel-watcher`
- `shm-fast-path` → `honker-core/shm-fast-path`

### `honker-extension`

- Latest observed version: `0.2.4`
- Published: 2026-06-05
- License: `MIT OR Apache-2.0`
- Edition: `2024`
- Declared `rust_version`: none observed in crates.io metadata
- Repository from crates.io: `https://github.com/russellromney/honker`
- Description: SQLite loadable extension that adds `honker_*` SQL functions for queues, streams, scheduler, and pub/sub.

Features observed for `0.2.4`:

- default: none
- `kernel-watcher`
- `shm-fast-path`

## Product shape

Honker provides durable messaging and scheduling inside a SQLite file:

- durable queues
- durable streams
- pub/sub / Postgres-style `NOTIFY` + `LISTEN`
- delayed jobs
- priority
- retries
- dead letters
- visibility timeouts
- heartbeat
- scheduler with cron / `@every`
- transactional enqueue / outbox semantics
- named locks
- rate limits
- job results

The product boundary is messaging and scheduling, not workflow orchestration or DAG execution. That aligns with Omegon: Honker can be a local async substrate, while Omegon owns assistant-run lifecycle semantics and Auspex owns remote execution boundaries.

## Rust packaging

Important finding: the current Rust binding docs say it does **not** require loading a runtime `.so` / `.dylib` extension.

The Rust language guide says the Rust binding registers Honker functions directly through `honker-core`. The docs.rs crate page for `honker 0.3.4` says the crate opens its own connection, registers every `honker_*` SQL function via `honker_core::attach_honker_functions`, bootstraps the schema, and needs no `.dylib` at runtime. Dynamic SQLite extension loading is mainly relevant to raw SQLite and non-Rust bindings.

There is documentation drift: the crates.io-rendered README still shows an older `Database::open("app.db", "./libhonker_ext.dylib")` shape and says the SQLite extension must be available at runtime. Treat the Rust language guide and docs.rs API docs as more authoritative for `0.3.4`, but verify with a minimal compile/run smoke before adoption.

Implication for Omegon: the largest packaging concern is reduced for the Rust-native path, though dependency/build compatibility still needs direct validation before adoption.

## Dependency compatibility and maturity

`honker 0.3.4` depends on:

- `honker-core ^0.2.4`
- `parking_lot ^0.12`
- `rusqlite ^0.39`
- `serde ^1`
- `serde_json ^1`
- `thiserror ^2`

The `rusqlite ^0.39` dependency is the main workspace compatibility item to test because Omegon already uses `rusqlite` in multiple crates and some paths rely on bundled SQLite. Multiple `rusqlite` versions can coexist at the Rust dependency level, but SQLite linkage and feature unification still need a real workspace build smoke.

Maturity signal is promising but early:

- `honker 0.3.4` has six published versions since 2026-04-20.
- docs.rs reports about 48% documentation coverage for `0.3.4`.
- crates.io download counts are low/new.
- upstream claims cross-platform CI for Rust core/extension and watcher backend proofs, but adoption should still be gated by local Omegon CI/build validation.

## Queue behavior

Honker queue jobs live in extension-managed SQLite tables. Claiming is atomic and uses visibility timeout semantics:

- job moves to processing on claim
- `claim_expires_at` allows reclaim if worker dies
- ack deletes completed jobs
- retry can delay and requeue
- dead-letter support exists
- heartbeat can extend the visibility window
- batch claim exists for throughput

Relevant SQL functions:

- `honker_enqueue(queue, payload, run_at_or_null, delay_or_null, priority, max_attempts, expires_or_null)`
- `honker_claim_batch(queue, worker_id, n, visibility_timeout_s)`
- `honker_ack(job_id, worker_id)`
- `honker_ack_batch(ids_json, worker_id)`
- `honker_retry(job_id, worker_id, delay_s, error)`
- `honker_fail(job_id, worker_id, error)`
- `honker_heartbeat(job_id, worker_id, extend_s)`
- `honker_sweep_expired(queue)`
- `honker_queue_next_claim_at(queue)`

This maps well to future assistant-run execution queues.

## Pub/sub behavior

Honker pub/sub is fire-and-forget and analogous to Postgres `NOTIFY`/`LISTEN`.

- low-latency cross-process wakeups on the same SQLite file
- no replay guarantees
- use streams for durable replay/per-consumer offsets
- `notify(channel, payload)` intentionally uses the Postgres-like function name

This maps well to waking local console/WebSocket/event-loop consumers, but should not be the durable run event history.

## Streams and scheduler

Streams provide durable pub/sub with per-consumer offsets and crash resume.

Relevant stream SQL functions:

- `honker_stream_publish(topic, key_or_null, payload)`
- `honker_stream_read_since(topic, offset, limit)`
- `honker_stream_save_offset(consumer, topic, offset)`
- `honker_stream_get_offset(consumer, topic)`

Scheduler supports cron and `@every` expressions with leader election.

Relevant scheduler SQL functions:

- `honker_scheduler_register(name, queue, cron_expr, payload, priority, expires_s)`
- `honker_scheduler_unregister(name)`
- `honker_scheduler_tick(now_unix)`
- `honker_scheduler_soonest()`
- `honker_cron_next_after(expr, from_unix)`

These are useful future fits for assistant run event subscriptions, scheduled assistants, and local worker wakeups.

## Remote pub/sub limitation

Honker is **single-machine/file-backed**. It coordinates processes that share the same SQLite file. It is not a remote distributed pub/sub system by itself.

For remote/Auspex-style execution:

- Honker can be the local ledger/queue/wakeup substrate.
- Auspex or another transport remains the remote executor/control-plane boundary.

## Testing guidance

Honker intentionally does not support in-memory SQLite filenames for production-faithful behavior. Upstream recommends temp file-backed databases for tests.

Implication: current plain `rusqlite` unit tests may use in-memory SQLite, but future Honker-backed tests should use `tempfile::tempdir()` and a real `.db` path.

## Architecture decision

Proceed with a plain SQLite assistant-run ledger now:

- `assistant_runs`
- `assistant_run_events`
- `rusqlite`
- WAL
- foreign keys
- busy timeout

Keep abstraction seams:

```rust
trait AssistantRunQueue {
    fn enqueue(&self, run_id: &str) -> anyhow::Result<()>;
    fn claim_next(&self, worker_id: &str) -> anyhow::Result<Option<AssistantRunQueueJob>>;
    fn ack(&self, job_id: &str) -> anyhow::Result<()>;
    fn retry(&self, job_id: &str, delay_s: u64, error: &str) -> anyhow::Result<()>;
}

trait AssistantRunEventBus {
    fn publish(&self, event: AssistantRunEvent) -> anyhow::Result<()>;
}
```

Initial implementations can be plain SQLite/no-op. Honker can later implement queue, stream, notify, and scheduler surfaces without forcing HTTP/ACP/Dioxus contract changes.

## Security requirements

Assistant-run storage and queue/event payloads must remain secret-safe:

- no secret values
- no raw environment variables
- no secret recipe payloads
- no raw prompts by default
- no unredacted logs
- progress strings must be pre-redacted before persistence/public projection

## Follow-up validation before adoption

Before adding Honker as a dependency, verify:

- license compatibility in the release review (`MIT OR Apache-2.0` looks acceptable)
- crate dependency graph
- crate version and maturity risk
- Rust 2024/toolchain implications
- missing declared MSRV / `rust_version`
- compatibility with workspace `rusqlite` / bundled SQLite
- whether `bundled-sqlite` feature is needed
- build behavior on macOS/Linux/Windows
- whether `kernel-watcher` or `shm-fast-path` are needed
- file-backed integration tests in this workspace

## Conclusion

Honker is a good architectural candidate and should remain the preferred future local queue/pubsub/scheduler backend. Adoption is deferred for dependency/build validation, not because of architectural mismatch.
