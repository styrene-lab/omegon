+++
id = "0f4610cf-380b-4673-a66e-e731c6ea7c30"
kind = "document"
title = "OpenAI Codex Responses API client — native Rust client for ChatGPT OAuth JWT tokens"
status = "implemented"
tags = ["providers", "openai-codex", "oauth", "rust", "sse", "responses-api", "0.15.1"]
aliases = ["openai-codex-responses-client"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
parent = "bridge-provider-routing"
+++

# OpenAI Codex Responses API client — native Rust client for ChatGPT OAuth JWT tokens

## Overview

Implement a Rust-native client for the OpenAI Codex Responses API, enabling ChatGPT Pro/Plus OAuth tokens (JWTs) to work natively in Omegon. The existing OpenAIClient uses Chat Completions (/v1/chat/completions) which rejects JWT tokens. The Codex Responses API uses a different endpoint (/codex/responses at chatgpt.com/backend-api), different message format (instructions/input instead of messages), and SSE streaming. Reference implementation: pi-ai's openai-codex-responses.js.

## Research

### Reference implementation analysis (pi-ai openai-codex-responses.js)

**Endpoint**: `POST https://chatgpt.com/backend-api/codex/responses` (SSE streaming)

**Auth**: JWT token → extract `chatgpt-account-id` from JWT payload at `payload["https://api.openai.com/auth"]["chatgpt_account_id"]`. Headers: `Authorization: Bearer {token}`, `chatgpt-account-id: {id}`, `originator: omegon`, `OpenAI-Beta: responses=experimental`.

**Request body** (different from Chat Completions): `instructions` (system prompt), `input` (message array), `model`, `stream: true`, `store: false`, `tools` (function format), `reasoning` (effort+summary), `tool_choice: auto`, `parallel_tool_calls: true`.

**Message format**: User → `{role: "user", content: [{type: "input_text", text}]}`, ToolResult → `{type: "function_call_output", call_id, output}`, Assistant text → `{type: "message", role: "assistant", content: [{type: "output_text", text}], id}`, Tool calls → `{type: "function_call", id, call_id, name, arguments}`.

**SSE events**: `response.output_item.added` (reasoning/message/function_call), `response.output_text.delta`, `response.reasoning_summary_text.delta`, `response.function_call_arguments.delta`, `response.output_item.done`, `response.completed` (with usage), `response.failed`, `error`.

**Tool call IDs**: Compound format `{call_id}|{item_id}`, item_id must start with "fc_", max 64 chars.

**Retry**: 429/5xx with exponential backoff (1s, 2s, 4s), max 3 retries.

## Decisions

### Decision: SSE-only transport (no WebSocket)

**Status:** decided
**Rationale:** The reference implementation supports both SSE and WebSocket transports. SSE is the default and sufficient for Omegon's use case. WebSocket adds significant complexity (connection pooling, session caching, idle timeouts) with marginal benefit. Can be added later if latency becomes an issue.

## Open Questions

*No open questions.*
