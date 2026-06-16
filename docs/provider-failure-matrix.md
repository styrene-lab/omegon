# Provider Failure and Degradation Matrix

This matrix records upstream error semantics that Omegon normalizes into `UpstreamErrorClass` before choosing retry, failover, reauth, context repair, or fatal stop behavior.

## Sources checked

Browser search was attempted through the transitional browser backend, but result extraction returned empty result sets while opening search pages. The first implementation slice therefore uses provider documentation/public API conventions already reflected in upstream SDK behavior and the current Omegon taxonomy, and it is structured so later source-cited updates can extend the table without changing the runtime contract.

Primary upstream documentation targets for follow-up citation:

- Anthropic API errors: `docs.anthropic.com/en/api/errors`
- OpenAI API error codes: `platform.openai.com/docs/guides/error-codes`
- Google Gemini troubleshooting / API errors: `ai.google.dev/gemini-api/docs/troubleshooting`
- Groq API errors: `console.groq.com/docs/errors`
- Mistral API docs and errors: `docs.mistral.ai`
- OpenRouter error handling / credits: `openrouter.ai/docs`
- xAI API docs: `docs.x.ai`
- Cerebras Inference docs: `inference-docs.cerebras.ai`

## Normalized classes

| Upstream signal | Providers | Omegon class | Recovery |
| --- | --- | --- | --- |
| `400`, `invalid_request_error`, invalid argument, unsupported parameter | all | `BadRequest` | fatal until request builder is fixed |
| `401`, invalid API key/token, expired token/session | all, especially OAuth-backed Codex/Anthropic | `AuthInvalid` / `SessionExpired` | reauthenticate |
| `403`, forbidden, permission denied, missing scope, organization role | all | `AuthInvalid` | reauthenticate / operator credential fix |
| `404`, model not found, unknown model | model APIs | `BadRequest` | fatal; registry/routing fix |
| `408`, request timeout | all | `Timeout` | retry same provider |
| `413`, request too large / context too long | Anthropic/OpenAI-compatible/Gemini | `ContextOverflow` | compact context |
| `422`, unprocessable entity / validation error | OpenAI-compatible providers, Mistral | `BadRequest` | fatal until payload fixed |
| `423`, locked / temporary resource contention | Groq-style APIs | `ProviderOverloaded` | failover preferred |
| `424`, failed dependency | Groq-style APIs | `Upstream5xx` | retry same provider |
| `429`, rate limit, resource exhausted, quota/rate cap | all | `RateLimited` unless quota/billing text is present | failover preferred |
| quota exhausted, insufficient credits, billing inactive | all | `QuotaExceeded` | fatal/operator billing fix |
| `498`, flex tier capacity/unavailable | Groq | `ProviderOverloaded` | failover preferred |
| `499`, client closed request/cancelled | proxy/OpenAI-compatible | `ResponseCancelled` | retry same provider if not operator-cancelled |
| `500`, `502`, `503`, `504`, `520`-`530`, overloaded, service unavailable | all | `Upstream5xx` or `ProviderOverloaded` | retry/failover depending class |
| Anthropic `529` / `overloaded_error` | Anthropic | `ProviderOverloaded` | failover preferred |
| Gemini `RESOURCE_EXHAUSTED` | Google | `RateLimited` or `QuotaExceeded` with billing/quota text | failover/fatal |
| Gemini `FAILED_PRECONDITION` | Google | `AuthInvalid` / `BadRequest` depending message | reauth/fatal |
| Responses API `response.incomplete` | Codex/OpenAI Responses | `ResponseIncomplete` | retry same provider |
| Responses API `response.cancelled` | Codex/OpenAI Responses | `ResponseCancelled` | retry same provider |
| SSE `error` with nested `{ error: { message, code, type } }` | Codex/OpenAI Responses | extracted detail then classified | class-specific |

## Runtime invariants

- Provider-specific parsing must preserve upstream status/code/type/message before reducing to an Omegon class.
- Unknown upstream error payloads must not render as bare `unknown error`; include a stable fallback that says which provider emitted an unrecognized payload.
- Retryable transport failures and provider degradation are separate from fatal request/auth errors.
- `429` with billing/credits/quota-exhaustion language is quota exhaustion, not a transient rate limit.
- OAuth subscription providers must prefer reauthentication guidance for expired/invalid sessions over generic provider failure prose.
