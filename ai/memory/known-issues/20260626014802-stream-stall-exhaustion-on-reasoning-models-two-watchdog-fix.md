+++
id = "9688ccb4-fd09-4c04-83cd-f775a27d7056"
title = "Stream stall exhaustion on reasoning models — two-watchdog fix"
tags = []
aliases = []
source_format = "omegon_memory"
source_path = "omegon://memory/Known Issues"
imported_at = "2026-06-26T01:48:02.764951Z"
imported_reference = true
kind = "memory_fact"
topic = "Known Issues"

[publication]
enabled = false
visibility = "private"

+++

`stream stall exhaustion: N consecutive stalled stream failures` on reasoning models (openai-codex:gpt-5.5, OpenAI o-series, Anthropic interleaved thinking) was caused by idle watchdogs misreading silent reasoning as a dead stream.

Two independent watchdogs exist: (1) PRODUCER — network SSE reader in providers.rs::process_sse via SsePhaseGate; (2) CONSUMER — agent event loop in loop.rs::consume_llm_stream. The CONSUMER is the one that counts stalls and triggers exhaustion (stall_exhausted when max_retries==0 and cumulative stalled time >= 600s). Fixing only the producer is insufficient.

Root cause (consumer): LlmEvent::ThinkingStart set received_content=true, collapsing the idle budget to the 90s content window; a model reasoning silently >90s tripped it, retries reached 600s bail.

Fix: both watchdogs are now phase-aware with three budgets — initial/active 90s, reasoning 300s. providers.rs: SsePhaseGate, env OMEGON_SSE_IDLE_TIMEOUT_SECS (90, min30) + OMEGON_SSE_REASONING_IDLE_TIMEOUT_SECS (300, min60). loop.rs: ThinkingStart/ThinkingDelta enter reasoning phase (no longer set received_content), TextStart/ToolCallStart clear it; pure helper select_stream_idle_budget; env OMEGON_LLM_REASONING_IDLE_TIMEOUT_SECS (300, min60).

Commits: fa63c9f3 (producer), 1444f7a7 (consumer). Full doc: design/stream-stall-watchdog.md. If stalls recur, treat as new bug: check elapsed_secs vs reasoning budget, confirm ThinkingStart fires, and whether a provider goes silent before ThinkingStart (initial phase) or after a text-start (active phase) — both are distinct unaddressed paths.
