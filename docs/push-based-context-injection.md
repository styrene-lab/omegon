---
id: push-based-context-injection
title: "Push-based pre-turn context injection — automatic shadow-context priming from incoming prompt"
status: exploring
parent: lifecycle-native-loop
tags: [context, shadow-context, ContextManager, performance, dx, prompt-injection]
open_questions:
  - "What is the right token budget for the auto-injected context block? Too small and it's not useful; too large and it consumes budget that conversation needs."
  - "Should the injected block be stable across turns (cached until the user prompt changes significantly) or re-scored every turn? Re-scoring every turn is more accurate but burns selector CPU on unchanged context."
  - "[assumption] The existing BM25 selector in shadow_context.rs is fast enough to run synchronously before the first LLM token without meaningful latency impact."
  - "Should auto-injected context be surfaced to the model as attributed (e.g. \\\"[context from memory]\\\") or silently prepended? Attributed = model knows it's synthetic; silent = cleaner prompt but the model may not discount it appropriately."
  - "Does injecting context before the model's first token cause any provider-specific formatting issues (e.g. Anthropic system prompt length limits, Codex system prompt truncation)?"
dependencies: []
related: []
---

# Push-based pre-turn context injection — automatic shadow-context priming from incoming prompt

## Overview

The model currently retrieves shadow-context on demand via `request_context`. The problem: the model doesn't know it needs context until it's already uncertain — by then it's either spray-reading files to orient itself, or making wrong assumptions. The harness sees the incoming prompt before the model does.

Proposed: intercept the incoming user message before tool dispatch, score it against the shadow-context corpus using the existing selector, inject the top 2–3 hits as a `[context]` block appended to the system prompt tail. The model starts each turn already oriented. `request_context` remains available for targeted follow-up retrieval; this handles baseline orientation automatically.

Costs:
- Tokens spent on context that might not be used (~1–3k tokens/turn)
- Selector noise: crude scoring may inject irrelevant chunks
- Latency: selector runs synchronously before first LLM token

Expected benefit:
- Eliminates tool-spray orientation pattern (model reads 3–5 files to reconstruct state)
- Reduces cold-start latency on session resumption
- Makes `request_context` a precision instrument rather than a crutch

The `lifecycle-native-loop` ContextManager already does phase-based injection (design node context, spec scenarios). This adds a second injection axis: prompt-scored corpus retrieval, independent of lifecycle phase.

## Open Questions

- What is the right token budget for the auto-injected context block? Too small and it's not useful; too large and it consumes budget that conversation needs.
- Should the injected block be stable across turns (cached until the user prompt changes significantly) or re-scored every turn? Re-scoring every turn is more accurate but burns selector CPU on unchanged context.
- [assumption] The existing BM25 selector in shadow_context.rs is fast enough to run synchronously before the first LLM token without meaningful latency impact.
- Should auto-injected context be surfaced to the model as attributed (e.g. \\"[context from memory]\\") or silently prepended? Attributed = model knows it's synthetic; silent = cleaner prompt but the model may not discount it appropriately.
- Does injecting context before the model's first token cause any provider-specific formatting issues (e.g. Anthropic system prompt length limits, Codex system prompt truncation)?
