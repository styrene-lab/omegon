+++
id = "64922132-b381-4813-ada6-ff749c740617"
kind = "document"
title = "Native local inference — Rust-native LLM execution without external runtime (Ollama/llama.cpp)"
status = "decided"
tags = ["architecture", "inference", "local", "rust", "burn", "candle", "ggml", "performance"]
aliases = ["native-local-inference"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
dependencies = ["rust-phase-3"]
issue_type = "feature"
open_questions = []
priority = "2"
+++

# Native local inference — Rust-native LLM execution without external runtime (Ollama/llama.cpp)

## Overview

Today Omegon delegates local inference to Ollama (separate process, HTTP API). A native Rust inference engine would eliminate that dependency — the binary itself runs models. This connects to Phase 3 (native providers) but for local models. Three candidates: Burn (tracel-ai), Candle (HuggingFace), and llama.cpp bindings.

## Research

### The Rust inference landscape — March 2026

Three viable approaches, each with different tradeoffs:

**1. Burn (tracel-ai/burn) + Burn-LM**
- Full Rust deep learning framework with JIT compiler (CubeCL)
- Burn-LM announced in alpha — dedicated LLM inference engine built on Burn
- Backends: ndarray (CPU), Metal (macOS), Vulkan, CUDA, ROCm/HIP, WebGPU
- Cross-platform by design — same model runs on any backend without changes
- Models: llama-burn exists in tracel-ai/models repo
- Pro: Pure Rust, portable, single binary story, no C/C++ dependencies
- Pro: JIT compiler auto-tunes for hardware — no manual kernel optimization
- Con: Alpha for LLM inference, performance not yet competitive with llama.cpp
- Con: Smaller model ecosystem than GGUF
- **Best for**: Long-term bet on a pure Rust stack. Aligns with Omegon's single-binary philosophy.

**2. Candle (huggingface/candle)**
- HuggingFace's minimalist ML framework for Rust
- Supports safetensors, GGML/GGUF quantized models, PyTorch format
- CPU + CUDA + Metal backends
- Crane project (lucasjinreal/Crane) builds a full inference engine on Candle — supports Qwen3, Llama, TTS, OCR
- Pro: Mature, production-used at HuggingFace, good GGUF support
- Pro: Minimalist — "make serverless inference possible"
- Con: HuggingFace-centric, less hardware portability than Burn
- Con: Less active development than Burn in 2026
- **Best for**: If we want GGUF model compatibility (same models as Ollama/llama.cpp) with a Rust-native runtime.

**3. llama.cpp bindings (`llama_cpp` crate)**
- Rust FFI bindings to llama.cpp (C/C++)
- Identical performance to llama.cpp — it IS llama.cpp
- Supports every GGUF model, every quantization format
- Pro: Proven, fast, huge model ecosystem
- Pro: Same models operators already use with Ollama
- Con: C/C++ dependency — complicates cross-compilation and single-binary story
- Con: Not pure Rust — burns the "zero dependencies" claim
- **Best for**: If we want drop-in Ollama replacement with minimal model compatibility risk.

**4. Keep Ollama (status quo)**
- Ollama is a separate process, HTTP API
- Pro: Already works, zero code to maintain, supports every model
- Con: Separate install, separate process, operator must manage it
- Con: Not portable — can't embed in a container image alongside omegon easily

**Assessment matrix:**

| Factor | Burn | Candle | llama.cpp | Ollama |
|---|---|---|---|---|
| Pure Rust | ✅ | ✅ | ❌ (FFI) | ❌ (separate) |
| Single binary | ✅ | ✅ | ⚠️ (static link) | ❌ |
| GGUF models | ❌ (own format) | ✅ | ✅ | ✅ |
| Metal (macOS) | ✅ | ✅ | ✅ | ✅ |
| CUDA (Linux) | ✅ | ✅ | ✅ | ✅ |
| Maturity for LLM | 🔴 Alpha | 🟡 Production | 🟢 Battle-tested | 🟢 Standard |
| Maintenance effort | Medium | Low | Low (bindings) | Zero |
| Fits single-binary story | ✅✅ | ✅ | ⚠️ | ❌ |

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
      │ qwen3:30b         30B params   context: 262k
      │ devstral:24b       24B params   context: 384k
      │ qwen3-embedding    0.6B params  embedding
    ○ Burn-LM     not available        (alpha, feature-gated)

  Routing:
    Context class: Maniple (272k)
    Thinking:      Medium (Magos)
    Tier:          Victory
    Local offload: embeddings → Candle, compaction → Ollama qwen3:30b
```

The key design points:

1. **Native (Candle)** shows as a separate backend from Ollama — it's embedded in the binary, always available if the feature flag is enabled, no external process needed.

2. **Ollama** probed via HTTP — same as today but now explicitly shown as an external backend alongside the native one.

3. **Burn-LM** shown as "not available" until the feature flag is enabled and Burn-LM matures. Placeholder to show the operator the roadmap.

4. **Routing section** shows where each task goes — embeddings route to native Candle (zero latency, in-process), compaction routes to Ollama for a larger model, cloud providers handle the main conversation.

5. The operator can override routing: `omegon config set local.embeddings native` vs `omegon config set local.embeddings ollama`. Default is native (Candle) when the feature is available, Ollama fallback when it's not.

This extends the existing operator-capability-profile bootstrap flow — it already probes providers and hardware. We add the inference backend tier to that probe.

## Decisions

### Decision: Layered inference: Candle for embeddings/small models now, Burn-LM for full inference when mature

**Status:** decided
**Rationale:** Candle (HuggingFace) is production-ready for embeddings and small models, supports GGUF, and is pure Rust. This replaces the Ollama dependency for lightweight tasks (memory semantic search, compaction summaries, memory extraction). Burn-LM is the long-term bet — pure Rust with JIT across Metal/Vulkan/CUDA/WebGPU — but is alpha for LLM inference. We watch and swap when ready. Ollama remains the heavy inference backend for large models until Burn-LM or Phase 3 native providers arrive.

### Decision: Native inference is a compile-time feature flag, not always included

**Status:** decided
**Rationale:** Embedding a model runtime adds significant binary size (Candle + model weights). Use cargo features: `omegon --features=local-inference` pulls in Candle. Default build stays lean (~12MB). Feature-gated build includes embedding model support. This keeps the install story clean for operators who only use cloud providers while enabling the single-binary local inference path for those who want it.

### Decision: Bootstrap probes and displays all inference backends — cloud, native (Candle), Ollama, Burn-LM

**Status:** decided
**Rationale:** The /bootstrap flow must show the operator the complete inference topology: which cloud providers are authenticated, which local backends are available (native Candle embedded, Ollama external, Burn-LM future), what models are loaded, and how routing is configured. Operator can override per-task routing (embeddings→native, compaction→ollama). Default: native for lightweight tasks when feature flag is present, Ollama fallback otherwise. This extends the existing operator-capability-profile probe.

## Open Questions

*No open questions.*
