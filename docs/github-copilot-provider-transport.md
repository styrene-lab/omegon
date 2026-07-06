---
id: github-copilot-provider-transport
title: "GitHub Copilot Provider Transport — direct inference target, not CLI dependency"
status: exploring
tags: [providers, github-copilot, routing, auth, transport]
open_questions:
  - "What stable GitHub-hosted inference endpoint is available to Copilot Enterprise clients outside the Copilot CLI?"
  - "Can Omegon acquire/refresh the required Copilot inference token without shelling out to or depending on the Copilot CLI?"
  - "Does the direct transport expose OpenAI-compatible chat/responses, streaming, and arbitrary tool calls?"
  - "Does GitHub expose a supported model-list/availability endpoint for enterprise Copilot inference?"
dependencies:
  - provider-route-conceptual-model-matrix
related:
  - github-copilot-first-class-provider
---

# GitHub Copilot Provider Transport — direct inference target, not CLI dependency

## Decision

The local `copilot` CLI is a cleanroom oracle only. Omegon must not ship, require, shell out to, or otherwise depend on the Copilot CLI for production GitHub Copilot inference.

The CLI is useful for evidence about model identifiers, route behavior, and error language. The implementation target remains a direct GitHub/Copilot transport owned by Omegon.

## Evidence from local cleanroom probe

The installed Copilot CLI confirms these Copilot-native model IDs:

- `gpt-5.5`
- `gpt-5.4`
- `claude-sonnet-4.6`
- `claude-opus-4.7`

It rejects these IDs:

- `claude-sonnet-4-6`
- `claude-opus-4.6`
- `claude-opus-4-6`
- `claude-opus-4-7`
- `gemini-2.5-pro`

This confirms provider model IDs are route-specific and cannot be derived by punctuation normalization from direct-provider IDs.

## Evidence from CLI documentation

`copilot help providers` describes BYOK/custom-provider mode, not the internal GitHub Copilot inference transport. The documented BYOK variables include:

- `COPILOT_PROVIDER_BASE_URL`
- `COPILOT_PROVIDER_TYPE`
- `COPILOT_PROVIDER_API_KEY`
- `COPILOT_PROVIDER_BEARER_TOKEN`
- `COPILOT_PROVIDER_WIRE_API`
- `COPILOT_PROVIDER_TRANSPORT`
- `COPILOT_PROVIDER_MODEL_ID`
- `COPILOT_PROVIDER_WIRE_MODEL`

This is useful because it shows the Copilot agent itself distinguishes model identity from wire model IDs, but it is not a supported path for Omegon to consume GitHub Copilot subscription inference.

Local logs reveal a GitHub MCP endpoint under `https://api.business.githubcopilot.com/mcp/readonly`, but that is MCP/tooling, not proven inference transport. It must not be treated as the chat/completions endpoint without separate evidence.

## Transport requirements for implementation

A shippable `github-copilot` provider bridge must provide:

1. Direct HTTP/WebSocket transport owned by Omegon.
2. Credential acquisition and refresh without requiring the Copilot CLI binary.
3. Request schema mapping into Omegon's `LlmBridge` abstraction.
4. Streaming support or explicit non-streaming degradation.
5. Tool-call support or explicit `toolDialect = none/unknown` route metadata.
6. Error normalization for auth, entitlement, invalid model, quota/rate limit, context overflow, and provider outage.
7. Model availability probing or a reviewed static registry with explicit probe command/tooling.

## Non-goals

- Do not implement a production bridge by invoking `copilot -p`.
- Do not treat Copilot routes as Anthropic/OpenAI credentials.
- Do not infer Copilot model IDs from direct-provider model IDs by string normalization.
- Do not claim same conceptual model means same context, tools, safety filters, quota, or commercial envelope.

## Next implementation slice

If direct transport evidence is found, implement:

- `GithubCopilotClient` transport skeleton.
- Auth resolver/token source for Copilot Enterprise inference.
- Text-only no-tool smoke path first.
- Route metadata that advertises actual transport/tool/streaming support.

If direct transport remains unproven, continue with provider-policy and semantic route resolution so the matrix becomes useful without prematurely coupling to the CLI.
