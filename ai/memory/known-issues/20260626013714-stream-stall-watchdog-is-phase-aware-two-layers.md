+++
id = "45b8d681-7212-46f6-a7f3-29012524afb6"
title = "Stream stall watchdog is phase-aware (two layers)"
tags = []
aliases = []
source_format = "omegon_memory"
source_path = "omegon://memory/Known Issues"
imported_at = "2026-06-26T01:37:14.523818Z"
imported_reference = true
kind = "memory_fact"
topic = "Known Issues"

[publication]
enabled = false
visibility = "private"

+++

"stream stall exhaustion: N consecutive stalled stream failures" on reasoning models (openai-codex:gpt-5.x, Anthropic interleaved thinking) was caused by flat 90s idle timeouts misreading silent reasoning as a dead stream. There are TWO watchdogs: producer-side process_sse (providers.rs) and consumer-side consume_llm_stream (loop.rs). The consumer is the one that counts stalls and bails at ~600s cumulative. The decisive bug: LlmEvent::ThinkingStart set received_content=true, collapsing the consumer budget to the 90s content window the moment reasoning began. Fixed (commits fa63c9f3 producer, 1444f7a7 consumer): both are now phase-aware with a generous reasoning budget (default 300s). Consumer reasoning phase keyed on ThinkingStart..first content/tool via pure helper select_stream_idle_budget. Env knobs: OMEGON_SSE_IDLE_TIMEOUT_SECS (90), OMEGON_SSE_REASONING_IDLE_TIMEOUT_SECS (300), OMEGON_LLM_INITIAL_IDLE_TIMEOUT_SECS (90), OMEGON_LLM_REASONING_IDLE_TIMEOUT_SECS (300). Full writeup: docs/stream-stall-watchdog.md. NOT verified live end-to-end; if stalls recur, capture the tracing line for which phase/budget was active — possible third path where provider emits text-start then goes silent.
