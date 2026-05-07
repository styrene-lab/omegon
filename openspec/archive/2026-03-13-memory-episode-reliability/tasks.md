+++
id = "71ff7c57-3ab2-42e5-93c3-5be45c35e8eb"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Memory: Episode Generation Reliability — Cloud fallback and guaranteed per-session narrative — Tasks

## 1. extensions/project-memory/extraction-v2.ts (modified)

- [x] 1.1 generateEpisode and generateEpisodeDirect — add fallback chain: local → codex-spark → haiku. Add buildTemplateEpisode() for telemetry-only fallback.

## 2. extensions/project-memory/index.ts (modified)

- [x] 2.1 Exit handler and session_shutdown — use new fallback-aware episode generation. Thread session telemetry (tool calls, files written) into episode generation call.

## 3. extensions/project-memory/types.ts (modified)

- [x] 3.1 MemoryConfig — add episodeModel, episodeTimeout, episodeFallbackChain fields.

## 4. Cross-cutting constraints

- [x] 4.1 Fallback chain must not extend shutdown beyond the existing shutdownExtractionTimeout budget — chain timeouts must sum to fit within it
- [x] 4.2 Template episode must be assembled from already-collected session telemetry, zero additional I/O
- [x] 4.3 Episode must always be emitted — null/undefined episode output is a bug, not a valid outcome
