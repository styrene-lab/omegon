+++
id = "52a10498-6091-47bb-8eb0-e575c8d00a80"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# OpenAI Codex Responses API client — native Rust client for ChatGPT OAuth JWT tokens — Design Spec (extracted)

> Auto-extracted from docs/openai-codex-responses-client.md at decide-time.

## Decisions

### SSE-only transport (no WebSocket) (decided)

The reference implementation supports both SSE and WebSocket transports. SSE is the default and sufficient for Omegon's use case. WebSocket adds significant complexity (connection pooling, session caching, idle timeouts) with marginal benefit. Can be added later if latency becomes an issue.

## Research Summary

### Reference implementation analysis (pi-ai openai-codex-responses.js)

**Endpoint**: `POST https://chatgpt.com/backend-api/codex/responses` (SSE streaming)

**Auth**: JWT token → extract `chatgpt-account-id` from JWT payload at `payload["https://api.openai.com/auth"]["chatgpt_account_id"]`. Headers: `Authorization: Bearer {token}`, `chatgpt-account-id: {id}`, `originator: omegon`, `OpenAI-Beta: responses=experimental`.

**Request body** (different from Chat Completions): `instructions` (system prompt), `input` (message array), `model`, `stream: true`, `store: false…
