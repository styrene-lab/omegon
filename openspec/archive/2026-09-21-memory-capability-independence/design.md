# Capability independence design

`omegon/src/setup.rs` composes independently resolved capabilities. The domain
crate continues compiling without the `agent` feature. Provider/model resolution
stays in the host; memory stores provider-neutral configuration/evidence identities
where needed. Avoid probing or initiating inference just to format status.

Use fake embedding and extraction implementations to test the availability matrix.
Keep extraction model selection outside the successful-embedding condition. Missing
credentials or provider failures produce typed extraction unavailability without
disabling storage. Embedding failures leave facts durable with repairable indexing
state. Retries are bounded and cancellation-aware. Readiness decoupling can land
independently; compatible indexing repair depends on memory-retrieval-contract.

Wave 3 decision: preserve the shipped Rust extraction default
`anthropic:claude-haiku-4-5-20251001`, expose an independent profile model override
and enable/disable switch, and use existing host completion routing. Parent sessions
configure extraction independently of embedding discovery; child sessions retain
their existing automatic-extraction-disabled policy. Configuration is not proof that
credentials or the provider are available.

The delta explicitly retires the historical GPT/subprocess and cloud-embedding-only
requirements. Existing profile-configured embedding discovery and lexical fallback
remain the embedding policy. No new provider or endpoint is introduced.

Expose readiness and degradation through existing semantic status and command
surfaces. Do not duplicate TUI-only policy. Query and extraction budgets use the
existing managed cancellation/lifecycle machinery.

Wave 5 readiness separates observed provider state from effective retrieval state.
`Configured` does not assert credentials, model identity, or successful inference.
Unknown indexing counts remain unknown. Storage outages disable effective retrieval
without erasing extraction or embedding observations. A pure semantic projection
combines cached observations, and serialized harness status includes that projection.
Status reads do not initiate provider requests or write the memory backend.

The shared identified-generation boundary selects cancellation before inference,
enforces an operation budget, and drops its owned future when interrupted. CLI
repair uses this boundary. The durable `index_fact` adapter commits an attempt
before inference and uses a fresh bounded token to settle cancellation or failure.
Automatic-generation hooks are listed in `integration-wave-5.md`.

Schema v14 stores one indexing attempt per fact, bound to its version, an attempt
ID, and the selected embedding space when verified identity is available. Unknown
identity remains explicit. Late success/failure callbacks cannot mutate a newer
attempt. Completion writes the compatible vector and removes pending state in
the same transaction, including its idempotency receipt. Failure does not modify
facts or reinforcement state. Managed status aggregates active attempts by typed
reason and projects version drift as SourceChanged without a durable write.

Repair starts a new attempt when selecting a recovered or changed embedding space.
It preserves ready vectors when their data is identical, clears obsolete pending
state, and leaves fact metadata unchanged. A failed completion transaction leaves
both the previous pending record and the previous vector state intact.

The optional local ONNX adapter owns a cancellation guard across `spawn_blocking`.
Dropping the caller signals a shared cancellation token and calls ONNX Runtime's
`RunOptions::terminate`. Queued work checks cancellation before inference, and
pooling checks it between tokens. Native termination is cooperative, not a forced
thread kill. Tests cover the shared signal and native-options lifetime without
claiming model-quality or real-model inference evidence.
