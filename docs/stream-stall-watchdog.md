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

## Update 2026-06-29 — completion guard parity (BridgeDropped)

A separate-but-related failure surfaced alongside the stall retries:

```
Upstream bridge dropped stream — retrying (attempt 1, delay 750ms): anthropic dropped the response stream before completion
```

This is a **different class** from a stall. `process_sse` returns `Ok(())`
whether the SSE byte stream ends cleanly (terminal event seen) **or** drops
mid-flight (`Ok(None)` → break). Only the Codex parser (`parse_codex_stream`)
guarded against the drop case; the Anthropic (`parse_anthropic_stream`) and
OpenAI/OpenRouter (`parse_openai_stream`) parsers did not. A drop *after*
partial content silently fed truncated text/tool-calls back into history as a
completed turn — the exact poisoning the Codex guard prevents.

### Fix

Both parsers now track a `completed` flag set on their terminal event
(`message_stop` for Anthropic, `finish_reason` for OpenAI). If the byte stream
ends with `completed == false`, the parser emits an `LlmEvent::Error`:

- with partial content → `"<provider>: stream closed without completion (had Nb text, M tool calls)"`
- with no content → `"<provider>: stream ended without a completion event"`

Both shapes match the existing `BridgeDropped` classifier substrings
(`"stream closed without completion"`, `"stream ended without"`), so they
classify as transient `BridgeDropped` → `RetrySameProvider`. Test:
`upstream_errors.rs::classify_anthropic_and_openai_incomplete_streams`.

## Update 2026-06-29 — full-surface adversarial sweep

A complete audit of the stream-stall surface across every provider found four
further defects beyond the codex re-arm and the Anthropic/OpenAI completion
guards. All are now fixed with tests.

### F0 (critical) — ambiguous-phase bail hard-failed instead of retrying

The consumer's ambiguous-reasoning-phase timeout message
(`"...had no observable activity ... or a stalled stream"`) matched none of the
`StalledStream` classifier substrings, so it fell through to `Unknown`, whose
`transient_kind()` is `None` → the loop treated it as fatal and did **not**
retry. This directly interacts with the active-output re-arm: a genuinely dead
stream re-armed into the ambiguous phase would hard-fail rather than retry.
Fix: added `"no observable activity"` and `"stalled stream"` to the
`StalledStream` rule. Test: `classify_all_provider_stall_and_drop_strings_as_transient`.

### F1 (high) — producer watchdog had no re-arm

`process_sse` (shared by Anthropic/OpenAI/codex) aborted on the *first*
active-phase idle. Because producer and consumer race at ~90s, the consumer
re-arm alone was insufficient — the producer could fire first and relay a
`Timeout`. `process_sse` now downgrades the gate to reasoning once on an
active-phase idle and keeps reading; only a second idle (reasoning phase) bails.
The bail message now reads `connection may be stalled` so it classifies as
`StalledStream`, unifying stall accounting across both watchdogs.

### F2 (medium) — Ollama NDJSON had no completion guard

`parse_ollama_ndjson_stream` emitted `Done` with whatever partial content it had
even when the stream ended without a `{"done":true}` chunk (mid-response drop on
ollama-cloud). Now tracks `saw_done` and emits a `BridgeDropped` error otherwise.

### F3/F4 (medium) — Antigravity/Gemini flat idle + no completion guard + double-emit

The Gemini (Cloud Code Assist) parser used a flat 90s idle (aborting reasoning
gaps), never checked `finishReason` (replaying truncated content as complete),
and emitted a `Done` even *after* it had already sent a timeout `Error`. Now: a
single re-arm to the reasoning budget on first idle (reset on activity), a
`finishReason` completion guard that emits `BridgeDropped` on a drop, and an
`aborted` flag that suppresses the trailing `Done` after any error.

### Out of scope / noted

- Ollama's producer has no idle timeout at all (relies on the consumer
  watchdog); a consumer-side abort can orphan the producer task until the HTTP
  layer times out. Local-only, low value — left as a known limitation.
- `stream_with_retry`'s `started` clock is never reset, so exhaustion measures
  cumulative wall-clock across retries. The re-arm fixes remove the dominant
  false-positive source; the cumulative semantics are intentional ("~10–20 min
  of cumulative stalls") and left as-is.
