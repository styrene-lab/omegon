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
