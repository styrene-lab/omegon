---
id: startup-systems-check
title: "Startup systems check — \\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\"I am Omegon, but where am I?\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\""
status: exploring
tags: [bootstrap, hardware, capabilities, inference, ux, 0.15.1]
open_questions:
  - "How do we detect GPU VRAM on macOS (unified memory / Metal) vs Linux (nvidia-smi / CUDA) vs no GPU, and what thresholds determine model sizing recommendations?"
  - "Should the systems check discover non-Ollama local inference endpoints (LM Studio, vLLM, text-generation-inference) via well-known ports or OpenAI-compatible /v1/models probing?"
  - Should the systems check result drive automatic routing decisions (e.g., auto-select local model for compaction if GPU is beefy enough) or just inform the operator?
jj_change_id: kvywttuknzuoxmkzorsyqmsrwqvwpnku
priority: 1
---

# Startup systems check — \\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\"I am Omegon, but where am I?\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\\"

## Overview

At startup, Omegon should probe its environment to understand what it can do: GPU presence and VRAM, Ollama reachability and loaded models, LM Studio or other local inference endpoints, available API keys (Anthropic, OpenAI), container runtimes, system resources. This drives routing decisions, tutorial content selection, and the bootstrap panel display. The question isn't just 'what providers are authenticated' — it's 'what kind of machine am I running on and what can I offer this operator?'

## Research

### Current bootstrap probe capabilities

The bootstrap panel (`tui/bootstrap.rs`) already probes:
- **Cloud providers**: Anthropic, OpenAI — checks for stored credentials and auth method
- **Local inference**: Ollama — checks reachability and lists loaded models
- **MCP servers**: Connection status, tool count, transport mode
- **Secrets**: Vault backend, locked/unlocked state, stored count
- **Container runtime**: Podman/Docker availability and version
- **Routing**: Context class, thinking level, capability tier
- **Memory**: Fact counts, episode count, edge count

What's missing:
- **Hardware profile**: GPU presence (Metal on macOS, CUDA on Linux), VRAM amount, total RAM, CPU cores. This determines what local models are feasible.
- **Network locality**: Is the user on a corporate network with internal inference endpoints? Is there a LM Studio or vLLM running somewhere?
- **Disk space**: Can we pull a 20GB model? How much space in ~/.ollama?
- **OS/arch**: Already known (build target) but not surfaced in the bootstrap panel.
- **Active model fit**: Given the hardware, which models from Ollama's library would actually run well? A 32B model on 8GB RAM is not "available" — it's a swap death sentence.

### Capability tier classification

The systems check should produce a `CapabilityTier` enum that downstream systems (tutorial, routing, bootstrap panel) consume:

```rust
enum CapabilityTier {
    /// Anthropic or OpenAI API key present. Full experience.
    FullCloud,
    /// Ollama running with 14B+ model, 32GB+ RAM. Full experience, local.
    BeefyLocal,
    /// No cloud key, but internet. Guide to Groq/Gemini free tier.
    FreeCloud,
    /// Ollama with small model (4B-8B). Abbreviated experience.
    SmallLocal,
    /// Nothing available. UI tour only.
    Offline,
}
```

Detection logic:
1. Check for Anthropic/OpenAI API keys → FullCloud
2. Check Ollama reachability → if running, list models:
   - Any model ≥14B params AND system RAM ≥32GB → BeefyLocal
   - Any model loaded but small → SmallLocal
3. Check for Groq/Gemini API keys → FreeCloud
4. Check internet connectivity → if reachable, could guide to FreeCloud
5. Fallback → Offline

The tier drives: tutorial variant selection, default routing policy, bootstrap panel messaging, and first-run guidance.

### OpenAI-compatible endpoint discovery

Omegon already supports `OPENAI_BASE_URL` for routing OpenAI client traffic to any compatible endpoint. This covers: Groq, LM Studio, vLLM, text-generation-inference, Ollama's OpenAI-compat endpoint, Together.ai, Fireworks.ai, and company-internal endpoints.

For the systems check, this means:
- If `OPENAI_BASE_URL` is set and not `api.openai.com`, probe it (`GET /v1/models`) and report available models
- Check well-known local ports: 11434 (Ollama), 1234 (LM Studio), 8080 (TGI/vLLM) — probe for OpenAI-compatible `/v1/models`
- Any discovered endpoint gets added to the bootstrap panel under "Local Inference" or "Custom Endpoints"

This makes Omegon work seamlessly with corporate internal model deployments without any configuration beyond an env var or a detected local port.

## Open Questions

- How do we detect GPU VRAM on macOS (unified memory / Metal) vs Linux (nvidia-smi / CUDA) vs no GPU, and what thresholds determine model sizing recommendations?
- Should the systems check discover non-Ollama local inference endpoints (LM Studio, vLLM, text-generation-inference) via well-known ports or OpenAI-compatible /v1/models probing?
- Should the systems check result drive automatic routing decisions (e.g., auto-select local model for compaction if GPU is beefy enough) or just inform the operator?
