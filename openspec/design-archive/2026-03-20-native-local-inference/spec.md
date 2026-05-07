+++
id = "0c511a03-bf02-4a38-8e80-aede73b74656"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Native local inference — Rust-native LLM execution without external runtime (Ollama/llama.cpp) — Design Spec (extracted)

> Auto-extracted from docs/native-local-inference.md at decide-time.

## Decisions

### Layered inference: Candle for embeddings/small models now, Burn-LM for full inference when mature (decided)

Candle (HuggingFace) is production-ready for embeddings and small models, supports GGUF, and is pure Rust. This replaces the Ollama dependency for lightweight tasks (memory semantic search, compaction summaries, memory extraction). Burn-LM is the long-term bet — pure Rust with JIT across Metal/Vulkan/CUDA/WebGPU — but is alpha for LLM inference. We watch and swap when ready. Ollama remains the heavy inference backend for large models until Burn-LM or Phase 3 native providers arrive.

### Native inference is a compile-time feature flag, not always included (decided)

Embedding a model runtime adds significant binary size (Candle + model weights). Use cargo features: `omegon --features=local-inference` pulls in Candle. Default build stays lean (~12MB). Feature-gated build includes embedding model support. This keeps the install story clean for operators who only use cloud providers while enabling the single-binary local inference path for those who want it.

### Bootstrap probes and displays all inference backends — cloud, native (Candle), Ollama, Burn-LM (decided)

The /bootstrap flow must show the operator the complete inference topology: which cloud providers are authenticated, which local backends are available (native Candle embedded, Ollama external, Burn-LM future), what models are loaded, and how routing is configured. Operator can override per-task routing (embeddings→native, compaction→ollama). Default: native for lightweight tasks when feature flag is present, Ollama fallback otherwise. This extends the existing operator-capability-profile probe.

## Research Summary

### The Rust inference landscape — March 2026

Three viable approaches, each with different tradeoffs:

**1. Burn (tracel-ai/burn) + Burn-LM**
- Full Rust deep learning framework with JIT compiler (CubeCL)
- Burn-LM announced in alpha — dedicated LLM inference engine built on Burn
- Backends: ndarray (CPU), Metal (macOS), Vulkan, CUDA, ROCm/HIP, WebGPU
- Cross-platform by design — same model runs on any backend without changes
- Models: llama-burn exists in tracel-ai/models repo
- Pro: Pure Rust, portable, single binary story, no C/C++ depen…

### Bootstrap integration — inference backends visible to operator

The `/bootstrap` (or `/init`) flow should probe and expose the full inference stack to the operator:

```
Ω  Omegon Bootstrap — Inference Backends

  Cloud Providers:
    ✓ Anthropic    Claude 4 Sonnet    (authenticated via OAuth)
    ✓ OpenAI       GPT-5.3            (API key found)
    ⚠ Copilot      not authenticated   → omegon login --github

  Local Inference:
    ✓ Native       Candle (embedded)   qwen3-embedding (384d)
    ✓ Ollama       running (v0.8.1)    3 models loaded
      │ qwen3:3…
