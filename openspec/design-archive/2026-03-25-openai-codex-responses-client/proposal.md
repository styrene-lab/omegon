+++
id = "0218ba75-4735-4590-90a0-919435419e6e"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# OpenAI Codex Responses API client — native Rust client for ChatGPT OAuth JWT tokens

## Intent

Implement a Rust-native client for the OpenAI Codex Responses API, enabling ChatGPT Pro/Plus OAuth tokens (JWTs) to work natively in Omegon. The existing OpenAIClient uses Chat Completions (/v1/chat/completions) which rejects JWT tokens. The Codex Responses API uses a different endpoint (/codex/responses at chatgpt.com/backend-api), different message format (instructions/input instead of messages), and SSE streaming. Reference implementation: pi-ai's openai-codex-responses.js.

See [design doc](../../../docs/openai-codex-responses-client.md).
