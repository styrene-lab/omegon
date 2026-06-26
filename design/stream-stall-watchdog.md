---
title: Stream Stall Watchdog — Reasoning-Aware Idle Budgets
status: implemented
tags: [providers, streaming, resilience, reasoning-models, known-issues]
---

# Stream Stall Watchdog

How Omegon decides a streaming LLM response has *stalled* (dead connection)
versus *legitimately reasoning* (alive but silent), and the fix that stopped
reasoning models from being killed mid-thought.

Related: [[agent-loop-resilience]], [[per-provider-input-limits]].

## Symptom

```
openai-codex stream stall exhaustion: 6 consecutive stalled stream failures over 652s.
The provider's stream is unresponsive. Retry later or switch provider with /model.
```

Seen on reasoning models — `openai-codex:gpt-5.5`, OpenAI o-series, Anthropic
interleaved thinking. The model was *not* dead; it was reasoning silently. The
watchdog misread silence as a stall, retried, re-stalled, and exhausted.

## There are two watchdogs, not one

This is the key fact for diagnosing any recurrence. A streamed turn passes
through two independent idle timers, in series:

| # | Layer | Location | Watches |
|---|-------|----------|---------|
| 1 | **Producer** — network SSE reader | `providers.rs::process_sse` via `SsePhaseGate` | Raw bytes off the wire |
| 2 | **Consumer** — agent event loop | `loop.rs::consume_llm_stream` | `LlmEvent`s on the mpsc channel |

The **consumer** (layer 2) is the one that counts consecutive stalls and
triggers `stream stall exhaustion` (`loop.rs`, `stall_exhausted` when
`max_retries == 0` and cumulative stalled time ≥ **600s**). Fixing only layer 1
is insufficient — layer 2 starves independently if no `LlmEvent` arrives.

## Root cause

Both watchdogs used a **flat idle budget that did not distinguish reasoning
from streaming**:

- Layer 1 (`process_sse`) had a flat 90s wire-idle timeout.
- Layer 2 (`consume_llm_stream`) treated `LlmEvent::ThinkingStart` as
  `received_content = true`, which dropped its idle budget to the **90s content
  window**. A reasoning model that then streamed nothing — not even
  reasoning-summary deltas — for >90s tripped the watchdog. Cumulative retries
  reached the 600s bail and surfaced as exhaustion.

Reasoning models routinely emit nothing on the wire for minutes. OpenAI's own
SDK default stream-idle is 5 minutes; the flat 90s budget was ~3× too tight for
the silent-reasoning phase.

## Fix — phase-aware budgets at both layers

Both watchdogs now carry **three phases** with a strictly longer leash while
reasoning:

| Phase | Budget (default) | Meaning |
|-------|------------------|---------|
| initial / pre-first-token | 90s | connected, nothing yet |
| **reasoning** | **300s** | thinking begun, no content/tool yet |
| active (content / tool tokens) | 90s | real output streaming |

### Layer 1 — `providers.rs`
`SsePhaseGate` flips on reasoning vs content/tool SSE events in each provider
closure (Anthropic, OpenAI chat, Codex Responses).

- `OMEGON_SSE_IDLE_TIMEOUT_SECS` — active budget, default **90**, min 30.
- `OMEGON_SSE_REASONING_IDLE_TIMEOUT_SECS` — reasoning budget, default **300**, min 60.

### Layer 2 — `loop.rs::consume_llm_stream`
`ThinkingStart` / `ThinkingDelta` now enter a distinct **reasoning** phase
(they no longer set `received_content`); `TextStart` / `ToolCallStart` clear it.
Budget selection is the pure, unit-tested `select_stream_idle_budget`.

- `OMEGON_LLM_INITIAL_IDLE_TIMEOUT_SECS` — initial budget.
- `OMEGON_LLM_REASONING_IDLE_TIMEOUT_SECS` — reasoning budget, default **300**, min 60.

## Commits

- `fa63c9f3` — fix: phase-aware SSE watchdog (layer 1).
- `1444f7a7` — fix: reasoning-aware consumer-side watchdog (layer 2, the actual stall counter).

## Recurrence playbook

If `stream stall exhaustion` reappears, treat it as a **new** bug and gather:

1. **Which layer fired?** Layer 2 logs `tracing::error!("stream stall exhaustion ...")`
   with `elapsed_secs`. If `elapsed_secs ≈ N × reasoning_budget`, the reasoning
   budget is now in play and may just be too short for that model → raise
   `OMEGON_LLM_REASONING_IDLE_TIMEOUT_SECS`.
2. **Did the phase transition fire?** Confirm the provider emits `ThinkingStart`
   for that model. If a provider goes silent *before* any `ThinkingStart`
   (still in the initial phase), the 90s initial budget applies — that is a
   third, distinct case and needs the initial budget widened or a
   `ThinkingStart` emitted earlier.
3. **Is it actually dead?** A genuinely unresponsive stream *should* still
   exhaust — the watchdog is meant to catch real death. Distinguish by whether
   the model eventually produces output when the budget is raised.

## Known limitation

If a provider streams a `MessageStart`/text-start and *then* goes silent before
any token (silence inside the active phase), the 90s active budget applies and
could still trip. Not observed in practice for current reasoning models, but it
is the next candidate failure path if stalls recur with content already begun.
