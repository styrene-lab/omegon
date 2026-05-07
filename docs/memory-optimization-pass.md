+++
id = "7f412398-58c6-43b5-bbee-d294912155a3"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Project Memory System Optimization

## Overview

Full optimization pass on the project-memory extension to eliminate unnecessary subprocess spawns, reduce per-turn overhead, and add proper concurrency controls. Motivated by 0.6.9 investigation showing runaway pi process accumulation.

## Research

### Subprocess spawn audit

8 `spawnExtraction()` call sites in extraction-v2.ts, each launching full Omegon runtime. Episode fallback chain spawns up to 3 sequential subprocesses per shutdown. Section pruning spawns one subprocess per oversized section at startup (fire-and-forget, no concurrency limit). Global extraction is a second subprocess per extraction cycle. The `runExtractionDirect()` function already demonstrates the correct pattern: direct HTTP to Ollama. Cloud models need the same treatment — direct fetch to Anthropic/OpenAI API instead of spawning a full runtime.

### Per-turn and concurrency overhead

Startup injection in session_start loads ALL facts to build proactive payload, then before_agent_start loads them again on first turn. Should be built lazily. No mutual exclusion between section pruning (fire-and-forget), background embedding indexing, and extraction cycles — only extraction has `isRunning` guard. `activeProc` is a single global variable; if pruning bypasses the isRunning flag, second spawn orphans the first. Trigger thresholds too aggressive: 8 tool calls / 5K tokens means extraction fires almost every task cycle.

## Decisions

### Decision: Replace spawnExtraction with direct HTTP for all LLM calls

**Status:** decided
**Rationale:** spawnExtraction launches a full Omegon runtime (node bin/omegon.mjs with extensions, session, etc.) just to make a single LLM chat completion. The existing runExtractionDirect() proves direct HTTP to Ollama works. Cloud models should use the same pattern — direct fetch to the provider API using keys from process.env. This eliminates all subprocess spawning from the memory system entirely.

### Decision: Add concurrency guard and deduplicate startup fact loading

**Status:** decided
**Rationale:** A single backgroundTaskRunning semaphore prevents pruning, indexing, and extraction from overlapping. Startup payload built lazily in before_agent_start (first turn) instead of session_start eliminates the double fact load. Trigger thresholds raised to 15 tool calls / 10K token delta to halve extraction frequency without losing coverage.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/project-memory/extraction-v2.ts` (modified) — Replace spawnExtraction with direct HTTP cloud chat function; eliminate all subprocess spawning; refactor episode/pruning to use direct calls
- `extensions/project-memory/llm-direct.ts` (new) — New module: direct HTTP chat for Anthropic and OpenAI APIs (no subprocess). Resolves API keys from env, provides chatDirect() with provider auto-detect and timeout/abort support
- `extensions/project-memory/index.ts` (modified) — Deduplicate startup fact loading (lazy in before_agent_start); add backgroundTaskRunning semaphore; raise extraction trigger thresholds; remove compaction subprocess fallback chain
- `extensions/project-memory/types.ts` (modified) — Raise default trigger thresholds (toolCallsBetweenUpdates: 15, minimumTokensBetweenUpdate: 10000)
- `extensions/project-memory/triggers.ts` (modified) — No structural changes, thresholds come from config

### Constraints

- Must not break existing Ollama direct path (runExtractionDirect)
- Cloud API keys resolved from process.env (ANTHROPIC_API_KEY, OPENAI_API_KEY) — no new config required
- Episode generation must always succeed (template fallback preserved)
- Section pruning must respect concurrency guard — no parallel subprocess spawns
