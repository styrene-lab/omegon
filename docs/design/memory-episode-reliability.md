+++
id = "210283f7-6fa5-4a3b-8691-6150d2dbea7c"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Memory: Episode Generation Reliability — Cloud fallback and guaranteed per-session narrative

## Overview

> Parent: [Memory System Overhaul — Reliable Cross-Session Context Continuity](memory-system-overhaul.md)
> Spawned from: "How do we fix episode generation reliability — cloud fallback chain, minimum viable template episode when all models fail, and ensuring at least one episode per session?"

*To be explored.*

## Decisions

### Decision: Episode generation uses the compaction fallback chain: Ollama → codex-spark → haiku

**Status:** decided
**Rationale:** Episode generation currently depends on Ollama at shutdown with a 15-second timeout. Episodes have been silent for 6 days as a result. The compaction fallback chain already exists and proven — reuse it. Try local first if available, fall back to codex-spark, then haiku. Same timeout budget as compaction.

### Decision: Always emit a minimum viable template episode when all models fail

**Status:** decided
**Rationale:** When every model in the fallback chain fails or times out, construct a skeleton episode from raw session telemetry: date, tool call count, files written (from write/edit calls), and topics extracted from tool call arguments. No model required — assembled deterministically. Better than silence. A model pass enriches it if available; the template is the floor not the ceiling.

### Decision: Episode generation chain is cloud-first, not Ollama-first

**Status:** decided
**Rationale:** Ollama-first was the wrong call for episode generation. Cloud retribution-tier (haiku, codex-spark) models are: (1) always available if the user can run pi at all — no separate install, no model pull, no RAM requirement; (2) substantially more capable for narrative generation than qwen3:30b; (3) cheap — a 500-token session summary on haiku costs ~$0.0001; (4) fast — sub-second API round-trip vs multi-second Ollama inference on consumer hardware. The 6-day episode silence was a direct consequence of Ollama-first. Cloud models should be primary; Ollama is an optional local preference, not a dependency. The principle: use local inference where the call is high-frequency and model quality threshold is low (embeddings). Use cloud where the call is low-frequency, quality matters, and the cost per call is trivial (episodes, extraction). Ollama is not a reliability upgrade — it is an availability downgrade for most users.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/project-memory/extraction-v2.ts` (modified) — generateEpisode and generateEpisodeDirect — add fallback chain: local → codex-spark → haiku. Add buildTemplateEpisode() for telemetry-only fallback.
- `extensions/project-memory/index.ts` (modified) — Exit handler and session_shutdown — use new fallback-aware episode generation. Thread session telemetry (tool calls, files written) into episode generation call.
- `extensions/project-memory/types.ts` (modified) — MemoryConfig — add episodeModel, episodeTimeout, episodeFallbackChain fields.

### Constraints

- Fallback chain must not extend shutdown beyond the existing shutdownExtractionTimeout budget — chain timeouts must sum to fit within it
- Template episode must be assembled from already-collected session telemetry, zero additional I/O
- Episode must always be emitted — null/undefined episode output is a bug, not a valid outcome
