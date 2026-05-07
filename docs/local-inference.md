+++
id = "ddd73e71-6d5a-4a15-87b6-40ec0639d30c"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Local Inference

> On-device model execution via Ollama — embeddings, extraction, compaction, and full agent sessions for cost-free operation.

## What It Does

The local inference subsystem manages Ollama for on-device model execution:

- **Model management**: `manage_ollama` tool (start/stop/status/pull), `list_local_models` tool
- **Task delegation**: `ask_local_model` tool sends prompts to local models with zero API cost
- **Offline driver**: `switch_to_offline_driver` swaps the driving model from cloud to local when connectivity fails
- **Embedding generation**: `qwen3-embedding` model powers semantic search in project memory
- **Background tasks**: Fact extraction, episode generation, and compaction can run on local models

Auto-selects the best available model: Nemotron 3 Nano (1M context), Devstral Small 2 (384K, code-focused), or Qwen3 30B (256K, general).

## Key Files

| File | Role |
|------|------|
| `extensions/local-inference/index.ts` | Extension entry — `manage_ollama`, `ask_local_model`, `list_local_models` tools |
| `extensions/offline-driver.ts` | Cloud → local driver switching, model auto-selection |
| `extensions/lib/local-models.ts` | Ollama model discovery, capability detection |

## Constraints & Known Limitations

- Requires Ollama installed and running locally
- GPU memory shared with FLUX.1 image generation — contention possible on smaller machines
- Local models have lower quality than cloud — best for boilerplate, transforms, and delegation
- Context windows vary significantly by model (4K to 1M tokens)

## Related Subsystems

- [Model Routing](model-routing.md) — effort tiers control local-vs-cloud ratio
- [Project Memory](project-memory.md) — embeddings and extraction use local models
- [Render](render.md) — FLUX.1 shares GPU with local inference
