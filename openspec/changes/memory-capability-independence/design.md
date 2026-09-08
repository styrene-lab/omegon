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

The baseline `memory/models.md` names `gpt-5.3-codex-spark`,
`text-embedding-3-small`, and a subprocess path; inspected Rust setup wires a
different extraction model and local/remote embedding implementations. Before
coding, decide whether to restore the existing default requirements or amend them
explicitly in this change. The present delta changes graceful degradation and
capability independence only. No model-default claim is considered verified yet.

Expose readiness and degradation through existing semantic status and command
surfaces. Do not duplicate TUI-only policy. Query and extraction budgets use the
existing managed cancellation/lifecycle machinery.
