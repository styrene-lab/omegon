---
id: rust-provider-fallback
title: Provider fallback chain — task-aware resilience in the Rust agent loop
status: deferred
parent: rust-agent-loop
related: [context-class-taxonomy-and-routing-policy]
tags: [rust, providers, resilience, fallback, routing]
open_questions:
  - "How does the fallback chain interact with the existing IntentDocument and thinking-level settings? If we fall back from Anthropic to OpenAI, thinking parameters don't translate 1:1."
  - "What defines 'task-aware'? Is it the current cognitive mode (design exploration vs code edit vs cleave child), the thinking level, or something the operator configures per-fallback?"
issue_type: feature
priority: 1
---

# Provider fallback chain — task-aware resilience in the Rust agent loop

## Overview

The Rust core currently has a single provider — when it fails, the user sees an error. Need a fallback chain that's smarter than blind retry. Key insight: fallback strategy should be task-aware. A cleave child doing mechanical edits can fall back to a cheaper/local model without losing much. A deep architecture discussion should retry the same tier or notify the operator rather than silently degrading to a model that can't reason at the same level. The chain wraps Arc&lt;dyn LlmBridge&gt; with ordered fallbacks. Each fallback attempt logs which provider was used. Transient errors (429, 500, 503) trigger retry+fallback; auth errors (401, 403) are terminal. The existing model-degradation node (implemented) covers the TS harness — this is the Rust-native equivalent, integrated with the provider abstraction in providers.rs.

## Decisions

### Decision: Lateral-first within tier, upgrade over downgrade, prefer tokens over thinking reduction

**Status:** decided
**Rationale:** Fallback priority: (1) try another provider at the same tier (e.g. Anthropic sonnet → OpenAI equivalent). (2) If no lateral option, upgrade to a higher tier at lower thinking rather than downgrade to a weaker model. Spending extra tokens on a capable model is preferable to degrading reasoning quality. (3) Only downgrade as last resort, and surface it to the operator. Never silently degrade on reasoning-heavy tasks.

## Open Questions

- How does the fallback chain interact with the existing IntentDocument and thinking-level settings? If we fall back from Anthropic to OpenAI, thinking parameters don't translate 1:1.
- What defines 'task-aware'? Is it the current cognitive mode (design exploration vs code edit vs cleave child), the thinking level, or something the operator configures per-fallback?
