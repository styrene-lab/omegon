+++
id = "c229c70e-baa0-49c6-9442-d1ddd0ddf4db"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Memory: Episode Generation Reliability — Cloud fallback and guaranteed per-session narrative — Design

## Architecture Decisions

### Decision: Episode generation uses the compaction fallback chain: Ollama → codex-spark → haiku

**Status:** decided
**Rationale:** Episode generation currently depends on Ollama at shutdown with a 15-second timeout. Episodes have been silent for 6 days as a result. The compaction fallback chain already exists and proven — reuse it. Try local first if available, fall back to codex-spark, then haiku. Same timeout budget as compaction.

### Decision: Always emit a minimum viable template episode when all models fail

**Status:** decided
**Rationale:** When every model in the fallback chain fails or times out, construct a skeleton episode from raw session telemetry: date, tool call count, files written (from write/edit calls), and topics extracted from tool call arguments. No model required — assembled deterministically. Better than silence. A model pass enriches it if available; the template is the floor not the ceiling.

## File Changes

- `extensions/project-memory/extraction-v2.ts` (modified) — generateEpisode and generateEpisodeDirect — add fallback chain: local → codex-spark → haiku. Add buildTemplateEpisode() for telemetry-only fallback.
- `extensions/project-memory/index.ts` (modified) — Exit handler and session_shutdown — use new fallback-aware episode generation. Thread session telemetry (tool calls, files written) into episode generation call.
- `extensions/project-memory/types.ts` (modified) — MemoryConfig — add episodeModel, episodeTimeout, episodeFallbackChain fields.

## Constraints

- Fallback chain must not extend shutdown beyond the existing shutdownExtractionTimeout budget — chain timeouts must sum to fit within it
- Template episode must be assembled from already-collected session telemetry, zero additional I/O
- Episode must always be emitted — null/undefined episode output is a bug, not a valid outcome
