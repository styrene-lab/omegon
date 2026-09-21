# Wave 5 integration handoff

## Host hooks owned by the parent

The readiness projection and cache are implemented in `surfaces/memory_status.rs`
and `status.rs`. Setup publishes independent configured/disabled/unavailable states.
`HarnessStatus.memory_capabilities` exposes the read-only projection to serialized
status consumers. Configuration is explicitly not successful inference evidence.

From normal extraction/embedding operations in `features/memory.rs` and
`features/memory/formation.rs`, publish content-free observations using:

```rust
use crate::surfaces::memory_status::{CapabilityReason, CapabilityState, ComponentReadiness};
crate::status::update_memory_capabilities(&root, |status| {
    status.extraction = ComponentReadiness {
        state: CapabilityState::Unavailable,
        reason: Some(CapabilityReason::ProviderUnavailable),
    };
});
```

Use `Ready`/`None` for successful inference, `Degraded`/`DeadlineExceeded` for
timeouts, `Degraded`/`Cancelled` for cancellation. Update `status.embeddings`
independently for identified embedding operations. Publish `pending_indexing`
from a bounded managed read/aggregate; `None` means not observed, never zero.
For memory-query details, include
`crate::status::memory_capabilities_snapshot_for(&self.status_root)` so callers
can inspect readiness without launching providers or mutating durable state.

## Durable indexing implementation and host hook

`crate::embedding::generate_identified(service.as_ref(), content, budget, &token)`
is now the shared bounded helper used by CLI repair. It returns typed
`BoundedEmbeddingError::{Cancelled, DeadlineExceeded, Generation(_)}` and drops
the owned inference future on cancellation/timeout. Use it directly for query
generation. For stored facts, use the durable helper below. Tests
cover timeout, cancellation after entry, and cancellation before entry without
wall-clock sleeps. Provider failures display a content-free diagnostic.
The optional local adapter now also signals cancellation into `spawn_blocking`
and calls `RunOptions::terminate` when its async caller is dropped.
The optional-feature focused run now passes, including managed cancellation,
durable reopen, and the local worker signal.

Replace the body of `features/memory.rs::persist_embedding` with a call to:

```rust
let outcome = crate::embedding::index_fact(
    embed_svc.as_ref(), binding, fact, &content, &operation_id, &cancellation,
).await;
```

The result is `IndexFactOutcome::{Indexed, Incomplete(reason), StorageUnavailable}`.
Publish the corresponding embedding observation to the readiness cache. Use Ready
for Indexed, Degraded/DeadlineExceeded for Timeout, Degraded/Cancelled for Cancelled,
Unavailable/ProviderUnavailable for Unavailable, and Degraded/IndexIncomplete for
other incomplete reasons. Use Unavailable/StorageUnavailable if persistence fails.
The helper never logs provider error content and never mutates fact confidence.

The helper commits pending state before inference. Cancellation/failure settlement
uses a fresh token and a two-second managed-write budget. Inference uses the
shared 30-second identified-generation budget. If settlement cannot reach storage,
the earlier pending record survives and remains retryable.

Schema v14 adds `embedding_indexing`, one outstanding record per fact. It records
the fact version, bounded attempt ID, optional verified embedding space, and typed
reason. Unknown space is explicit until verified identity exists. The mutations are:

- `RecordEmbeddingIndexing { record }`: Pending begins/replaces an attempt;
  failure updates require the same fact version, attempt ID, and selected space.
- `CompleteEmbeddingIndexing { fact, attempt_id, embedding }`: requires the same
  attempt and a compatible space, then atomically writes the vector and clears
  pending state. Late callbacks cannot replace newer attempts.
- Existing `StoreIdentifiedEmbedding` also clears compatible pending state
  atomically. Explicit repair starts a new attempt to select a changed space.

`MemoryRequestV1::EmbeddingIndexingRecord` reads a single record.
`ManagedMemoryStatusV1.indexing` exposes aggregate pending/retryable/terminal counts,
untracked missing/legacy vectors, and typed reasons. Version drift projects as
SourceChanged without a write. Startup and normal status refresh already publish
this aggregate into `HarnessStatus.memory_capabilities.indexing` and pending counts.

CLI repair uses these mutations, persists generation failures, and completes
existing ready vectors without rewriting identical vector data or reinforcing facts.

Wave 4 already verifies explicit repair and no reinforcement; see
`../2026-09-21-memory-retrieval-contract/verification-wave-4.md`. Scheduling new automatic
retry jobs is not required by the delta: bounded explicit repair is the recovery
action, provided pending state and failure reasons are durable and observable.
