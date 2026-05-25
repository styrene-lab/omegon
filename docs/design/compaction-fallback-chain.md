+++
id = "d50906c8-b75e-4872-9e8b-10a02675c43c"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Intelligent Compaction Fallback Chain — Local → GPT-5.3 → Haiku

## Disposition — 2026-05-23

**Status: historical-decision / stale implementation scope.** This node describes compaction interception through the older TypeScript `extensions/project-memory/*` and `extensions/lib/model-routing.ts` architecture. Those paths are absent in the current Rust-native repository; current context compaction is implemented through Rust conversation/control-runtime paths such as `core/crates/omegon/src/conversation.rs` and `core/crates/omegon/src/control_runtime.rs`.

Preserve the reliability principle: compaction handoff quality matters, local-only compaction can fail poorly, and fallback chains should be bounded. Do not use the `compactionLocalFirst`, `session_before_compact`, or TypeScript file-scope details as current implementation guidance without a Rust reconciliation pass.

## Overview

Replace the current binary local/cloud compaction choice with an intelligent fallback chain that prioritizes quality and resilience. Local models timeout frequently; current cloud fallback uses whatever driver model is active. Instead: local → gpt-5.3-codex-spark → haiku → sonnet, with each step having appropriate timeouts and quality expectations.

## Research

### Current compaction architecture weaknesses

**Binary choice problem**: compactionLocalFirst=true routes ALL compaction to Ollama direct HTTP with 120s timeout. On timeout/failure, there is no intelligent cloud fallback — it fails entirely.

**No dedicated compaction model selection**: When compaction falls through to pi core, it uses `this.model` (current driver) rather than a dedicated compaction-optimized model choice.

**Provider-agnostic gap**: The effort tier system has `resolveExtractionTier()` that can upgrade local→haiku when cheapCloudPreferredOverLocal=true, but no equivalent `resolveCompactionTier()`.

**Quality vs cost tradeoff poorly handled**: No way to prefer free high-quality models (gpt-5.3-codex-spark at $0) over paid lower-quality models (haiku) when local times out.

**Critical for handoff scenarios**: User reports compaction timeouts are "not ideal" because "giving the continuation to the new context window after the handoff is critical" — current architecture provides insufficient reliability for this use case.

### Pi core compaction implementation

**Pi core compaction flow**: `agent-session.js` calls `compact(preparation, this.model, apiKey, customInstructions, signal)` where `this.model` is the current session driver model.

**Reasoning-aware**: If the model has `reasoning: true`, pi core compaction uses `reasoning: "high"` via `completeSimple()`.

**Extension interception**: `session_before_compact` event allows extensions to provide custom compaction via `{ compaction: { summary, firstKeptEntryId, tokensBefore, details } }`.

**Current project-memory intercept**: `extensions/project-memory/index.ts` intercepts ALL compaction when `compactionLocalFirst=true` (default), routing to `ollamaChat()` with 120s timeout, 60k char truncation, and `num_ctx: 32768`.

**Integration strength**: The model-routing infrastructure in `extensions/lib/model-routing.ts` already maps abstract tiers to concrete provider models. OpenAI **sonnet-class** maps to **`gpt-5.3-codex-spark`** ($0 with reasoning). This is used by model-budget and effort for driver selection, but not for compaction.

## Decisions

### Decision: Intelligent fallback chain architecture

**Status:** decided
**Rationale:** Implement a provider-aware compaction fallback chain within the existing session_before_compact interception: (1) Local model with 45s timeout, (2) gpt-5.3-codex-spark with 60s timeout, (3) Haiku with 30s timeout, (4) Allow pi core fallback to current driver model. Each tier uses appropriate reasoning settings and timeout budgets. Preserves existing effort tier behavior while adding intelligent cloud fallbacks for reliability.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/project-memory/index.ts` (modified) — Add resolveCompactionTier() function and implement fallback chain in session_before_compact handler
- `extensions/project-memory/types.ts` (modified) — Add compactionFallbackChain config option and timeout settings per tier
- `extensions/project-memory/index.ts` (modified) — Added resolveCompactionFallbackChain() and intelligent session_before_compact handler
- `extensions/project-memory/types.ts` (modified) — Added compactionFallbackChain, compactionCodexTimeout, compactionHaikuTimeout config options

### Constraints

- Preserve existing compactionLocalFirst behavior for backward compatibility
- Use existing model-routing.ts infrastructure for provider-aware resolution
- Respect effort tier overrides — tiers 1-5 prefer local, tiers 6-7 can start with cloud
- Each fallback tier must have its own timeout to prevent cascade failures
- Must work when cheapCloudPreferredOverLocal policy is active
- Preserve backward compatibility with compactionLocalFirst behavior
- Use model-routing.ts for provider-aware resolution
- Graceful fallthrough to Pi core for cloud models
