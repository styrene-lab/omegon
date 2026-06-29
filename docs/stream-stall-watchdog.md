---
title: Stream Stall Watchdog — Phase-Aware Idle Timeouts
status: implemented
tags: [providers, streaming, reasoning, watchdog, reliability]
date: 2026-06-25
---

# Stream Stall Watchdog — Phase-Aware Idle Timeouts

## Summary

Reasoning models (OpenAI `gpt-5.x` / o-series, Anthropic interleaved thinking)
can stream **nothing on the wire for minutes** during legitimate reasoning —
not even reasoning-summary deltas. Omegon's stream watchdogs used flat idle
timeouts that could not tell silent-thinking apart from a dead connection, so
they aborted live reasoning turns. Repeated aborts hit the cumulative
stall-exhaustion bail and surfaced to the operator as:

```
openai-codex stream stall exhaustion: 6 consecutive stalled stream failures over 652s.
The provider's stream is unresponsive. Retry later or switch provider with /model.
```

The fix makes **both** stream watchdogs phase-aware: a tight budget while
content/tool tokens actively stream, and a generous budget while the model is
reasoning or before the first token.

## The two watchdogs (this was the trap)

There are two independent idle timers on the streaming path. The first fix
patched only the producer; the symptom persisted because the **consumer** is
the layer that actually counts stalls and bails.

| Layer | Location | Watches | Role |
|-------|----------|---------|------|
| **Producer** | `core/crates/omegon/src/providers.rs` → `process_sse` | Raw network SSE bytes | Translates SSE → `LlmEvent`s |
| **Consumer** | `core/crates/omegon/src/loop.rs` → `consume_llm_stream` | The `LlmEvent` channel | Builds the assistant message; **counts stalls** |

The consumer's abort is classified as `TransientFailureKind::StalledStream`.
After ~600s of cumulative stalls (`loop.rs`, `stall_exhausted`) the loop bails
with `"stream stall exhaustion"`.

## Root cause

### Producer side (`process_sse`)
A flat 90s idle timeout terminated the SSE read during silent reasoning, even
though the connection was alive.

### Consumer side (`consume_llm_stream`) — the decisive bug
`consume_llm_stream` had a two-phase budget: an *initial* window before any
event and a *content* window (90s) after "content received". The bug:
`LlmEvent::ThinkingStart` set `received_content = true`. The moment a reasoning
model began thinking, the budget collapsed to the **90s content window**, then a
silent reasoning gap of >90s tripped it. Each abort retried into the same wall;
cumulative stalls reached the 600s exhaustion bail.

This is why the first (producer-only) fix did not resolve the symptom: the
consumer watchdog was still flat at 90s and is the layer that exhausts.

## The fix

Both watchdogs now distinguish three phases and apply a strictly longer leash
while reasoning.

### Producer — `process_sse`
A `SsePhaseGate` (atomic) is flipped by each provider closure:

- `gate.reasoning()` on reasoning items / reasoning-summary deltas / between
  output items (Codex Responses), on `thinking` blocks and `content_block_stop`
  (Anthropic).
- `gate.active()` on text/message/function-call/tool-use deltas.
- Defaults to the reasoning (generous) phase pre-first-token.

Budgets:
- **active**: `OMEGON_SSE_IDLE_TIMEOUT_SECS` — default **90s**, min 30.
- **reasoning**: `OMEGON_SSE_REASONING_IDLE_TIMEOUT_SECS` — default **300s**, min 60.

### Consumer — `consume_llm_stream`
A `reasoning` flag (separate from `received_content`) is set on
`ThinkingStart` / `ThinkingDelta` and cleared on `TextStart` / `ToolCallStart`.
`ThinkingStart` no longer marks `received_content`. Budget selection is the pure
helper `select_stream_idle_budget(reasoning, received_content, initial, content,
reasoning)`:

- reasoning && !received_content → **reasoning budget**
- received_content → **content budget**
- otherwise → **initial budget**

Budgets:
- **initial**: `OMEGON_LLM_INITIAL_IDLE_TIMEOUT_SECS` — default 90s, min 30.
- **content**: 90s.
- **reasoning**: `OMEGON_LLM_REASONING_IDLE_TIMEOUT_SECS` — default **300s**, min 60.

## Research basis for 300s

Not a guess. OpenAI's own SDK ships a 15-minute request timeout and
high-reasoning-effort streams routinely go silent for minutes. Anthropic SDK
issues (#998 ping-aware watchdog, #867 "still thinking vs stalled") describe the
same failure mode. 300s clears observed silent-reasoning gaps while still
catching a genuinely dead stream inside the retry budget.

## Tests

- `providers.rs`: `sse_phase_gate_defaults_to_reasoning`,
  `sse_phase_gate_transitions`, `sse_idle_budget_reasoning_exceeds_active`,
  `sse_idle_budget_defaults_are_research_backed`, `env_secs_floor_and_parse`.
- `loop.rs`: `stream_idle_budget_is_phase_aware` — asserts reasoning-before-content
  gets the generous budget (the exact regression), content takes precedence over a
  stale reasoning flag, and reasoning > content.

## Commits

- `fa63c9f3` — producer-side `process_sse` phase-aware watchdog.
- `1444f7a7` — consumer-side `consume_llm_stream` reasoning-aware watchdog (the
  decisive fix).

## Operator knobs

| Env var | Layer | Default | Min |
|---------|-------|---------|-----|
| `OMEGON_SSE_IDLE_TIMEOUT_SECS` | producer active | 90 | 30 |
| `OMEGON_SSE_REASONING_IDLE_TIMEOUT_SECS` | producer reasoning | 300 | 60 |
| `OMEGON_LLM_INITIAL_IDLE_TIMEOUT_SECS` | consumer initial | 90 | 30 |
| `OMEGON_LLM_REASONING_IDLE_TIMEOUT_SECS` | consumer reasoning | 300 | 60 |

## Known limitations / future bugs to watch

- **Unverified end to end.** The mechanism the error points to is fixed with
  passing unit tests, but a live 90s+ silent-reasoning stall was not reproduced.
  If stalls recur, capture the `tracing` line to see which phase/budget was
  active — a recurrence could indicate a *third* path (e.g. provider emits
  `MessageStart`/text-start, then goes silent, landing in the content budget).
- The consumer reasoning phase keys off `ThinkingStart`. A provider that reasons
  without ever emitting `ThinkingStart` would fall back to the initial/content
  budgets. Not observed for the Responses API (reasoning `output_item.added`
  arrives promptly), but it is the most likely blind spot.

## Update 2026-06-29 — the third path (active-output re-arm)

The predicted "third path" above was confirmed in the field with
`openai-codex:gpt-5.5`: the provider streams **text/tool deltas for one output
item**, then pauses for minutes while deciding the next item — *without* first
emitting a `TextEnd`/`ToolCallEnd`. The phase therefore stays
`OutputStreaming`/`ToolStreaming`, the tight 90s active budget fires, and the
operator sees a spurious:

```
Upstream stalled stream — retrying (attempt 2, delay 1500ms): openai-codex stream stopped producing output
```

### Fix

The consumer (`consume_llm_stream`) now **re-arms once** instead of aborting on
the first active-output silence. `rearm_idle_phase(phase)` returns
`Some(AmbiguousSilent)` for `OutputStreaming`/`ToolStreaming` and `None`
otherwise. On the first timeout in an active phase the watchdog:

1. stores `AmbiguousSilent` (picks up the generous reasoning budget),
2. emits a non-fatal `StreamIdle{ ambiguous: true }` breadcrumb,
3. `continue`s the recv loop — it does **not** abort or count a stall.

Any resumed delta flips the phase back to active via
`stream_idle_phase_after_event`. A *second* silence is now evaluated in
`AmbiguousSilent`, which does not re-arm, so a genuinely dead stream still dies
inside the retry budget. `AwaitingFirstEvent` does **not** re-arm — pre-first-token
silence is a connection problem, not a reasoning gap.

Net effect: the legal inter-item reasoning gap gets one tight budget + one
reasoning budget (~90s + ~600s) before any stall is counted, while a dead
connection costs at most one extra tight budget before it surfaces.

- `loop.rs`: `active_output_silence_rearms_to_reasoning_budget` — asserts the
  active phases re-arm to the reasoning budget exactly once, and that ambiguous,
  reasoning, and awaiting-first-event phases do not re-arm.
