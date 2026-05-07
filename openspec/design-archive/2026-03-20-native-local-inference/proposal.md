+++
id = "299dc95e-27de-4477-bfe1-003cdc9796c3"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Native local inference — Rust-native LLM execution without external runtime (Ollama/llama.cpp)

## Intent

Today Omegon delegates local inference to Ollama (separate process, HTTP API). A native Rust inference engine would eliminate that dependency — the binary itself runs models. This connects to Phase 3 (native providers) but for local models. Three candidates: Burn (tracel-ai), Candle (HuggingFace), and llama.cpp bindings.

See [design doc](../../../docs/native-local-inference.md).
