---
title: NAI Agent Gateway Compatibility MVP
status: exploring
tags: [nai, nutanix, providers, openai-compatibility, agent-runtime, mvp]
---

# NAI Agent Gateway Compatibility MVP

## Purpose

Define the smallest Omegon-based agent MVP that can consume models exposed through the Nutanix AI (NAI) Agent Gateway after the gateway is installed and operational.

This design begins at the gateway boundary. NKP/Kommander cluster deployment, GPU enablement, model serving, and NAI platform installation are upstream platform concerns and are deliberately outside this node.

## Architectural boundary

```text
NAI-managed model deployment and governance
                    |
                    | OpenAI-compatible chat/tool-calling API
                    v
           NAI Agent Gateway endpoint
                    |
                    | HTTPS + scoped enterprise credential
                    v
        Omegon OpenAI-compatible provider
                    |
                    +-- agent loop and reasoning
                    +-- tool schema publication
                    +-- streamed response handling
                    +-- tool execution and result return
                    +-- session and lifecycle controls
```

### Ownership

NAI owns:

- Model deployment, runtime selection, and GPU scheduling.
- The unified model endpoint and its network availability.
- Endpoint/model authorization, API credentials, quotas, and rate limits.
- Model names exposed through the gateway.
- Gateway-side audit, observability, and policy enforcement.

Omegon owns:

- Agent orchestration after the gateway.
- System and user prompt construction.
- OpenAI-compatible request and stream handling.
- Tool-schema publication, local tool execution, and tool-result continuation.
- Session state, bounded execution, retries, and operator-visible diagnostics.
- Tool-level authorization for capabilities controlled by the agent runtime.

The integration must not imply that Omegon RBAC directly integrates with KServe or Kubernetes RBAC. Omegon authenticates to an NAI endpoint; NAI applies its own endpoint and model policy, while Omegon separately governs local agent tools.

## Current Omegon fit

Omegon already contains most of the required agent-runtime path:

- `OpenAIClient` serializes system, user, assistant, and tool-result messages.
- It publishes tools using the OpenAI function-calling shape.
- It posts streaming requests to `{base_url}/v1/chat/completions`.
- `OpenAICompatClient` wraps this implementation for providers using the OpenAI Chat Completions protocol.
- The provider layer already parses streamed content and tool calls into the provider-neutral `LlmEvent` interface used by the agent loop.
- `OPENAI_BASE_URL` proves that base-URL substitution is already supported for the native OpenAI provider.

The current gap is configuration and identity, not a new agent framework. `resolve_provider` accepts a fixed set of provider IDs, and `compat_base_url` supplies static URLs for known compatible providers. NAI therefore cannot yet be represented cleanly as a named, independently configured enterprise endpoint without either overloading the `openai` provider or adding a first-class endpoint registration path.

## MVP decision

Use Omegon directly as the post-gateway agent runtime. Do not add LangChain or LangGraph to the MVP.

Those frameworks would duplicate capabilities Omegon already owns—agent loops, tool calls, state, retries, and bounded execution—while introducing a second runtime and dependency graph. They remain candidates only if a later use case requires a framework-specific library or graph semantic that Omegon demonstrably lacks.

The MVP should add a named `nai` OpenAI-compatible provider backed by explicit endpoint configuration and a scoped secret. This is the smallest change that preserves provider identity in model specs, telemetry, diagnostics, and policy instead of disguising NAI as public OpenAI.

## Proposed operator contract

Minimum configuration:

```bash
export NAI_BASE_URL="https://<nai-agent-gateway>/<endpoint-root>"
export NAI_API_KEY="<scoped-gateway-credential>"
omegon --model "nai:<gateway-model-id>"
```

The configured base URL must be the root immediately preceding `/v1/chat/completions`, because the existing client appends that path. The exact customer URL and model identifier must be discovered from the deployed NAI environment; they must not be fabricated or compiled into Omegon.

A durable inference manifest should be the follow-on configuration surface:

```toml
schema_version = 1

[[endpoints]]
id = "nai-medtronic"
adapter = "openai-chat-completions"
secret_refs = ["NAI_API_KEY"]
enabled = true

[endpoints.transport]
kind = "http"
base_url = "https://<nai-agent-gateway>/<endpoint-root>"

[[offerings]]
id = "nai-medtronic/<gateway-model-id>"
endpoint = "nai-medtronic"
native_model_id = "<gateway-model-id>"
enabled = true
```

The manifest shape already exists in Omegon's inference inventory, but runtime bridge construction from arbitrary manifest endpoints must be verified or completed before it can replace the environment-variable MVP.

## Required implementation

### 1. Named provider registration

Register `nai` as an OpenAI-compatible provider and preserve `nai:<model>` as the model-spec identity throughout routing and telemetry.

Required behavior:

- Resolve credentials only from the NAI secret recipe/environment variable.
- Read `NAI_BASE_URL`; do not ship a default external endpoint.
- Reject missing or non-HTTP(S) endpoint configuration with a clear diagnostic.
- Avoid logging API keys or authorization headers.
- Do not silently fall back from NAI to a public model provider for an explicitly selected `nai:*` model unless the operator has configured such a policy.

### 2. Chat and streaming compatibility

Use the existing OpenAI-compatible request path and verify NAI accepts:

- `POST /v1/chat/completions`
- Server-sent event streaming
- Standard chat roles
- The gateway's model identifier
- OpenAI function/tool declarations
- Assistant tool-call deltas
- Tool-result messages correlated by tool-call ID

Compatibility must be tested against the actual NAI gateway. “OpenAI-compatible” does not guarantee support for every optional field or streaming detail.

### 3. Capability shaping

Start conservatively:

- Text input and output are required.
- Streaming is required for the interactive MVP.
- Tool calling is required for the agentic MVP.
- Images, embeddings, structured output, and provider-specific reasoning controls are out of scope until demonstrated by the selected NAI endpoint.
- Do not send `reasoning_effort` or other optional OpenAI fields unless the gateway/model advertises and accepts them.

If the selected model can chat but cannot issue tool calls, it may be exposed as a degraded chat-only offering, but it does not satisfy the agentic MVP acceptance criteria.

### 4. Credential and network handling

The credential represents NAI gateway authority, not an OpenAI account.

- Store it through Omegon's secrets system or an environment variable, never in a project document or committed manifest.
- Use TLS verification by default.
- Support enterprise CA configuration through the established host/runtime trust mechanism rather than disabling certificate checks.
- Treat `401` and `403` as gateway credential/policy failures.
- Treat `404` as a likely base-path or model-routing mismatch.
- Treat `429` as gateway quota/rate limiting and surface retry metadata when available.
- Distinguish gateway/network failures from local tool-execution failures in operator diagnostics.

### 5. Model discovery

Discovery is useful but not required for the first vertical slice. The MVP may accept an operator-supplied model ID.

If NAI exposes OpenAI-compatible `GET /v1/models`, Omegon should use its existing OpenAI discovery semantics and record returned model IDs as gateway evidence. If the gateway does not expose that endpoint or filters it by credential, model IDs must come from NAI endpoint configuration rather than guesswork.

## Vertical-slice demonstration

The first demonstration should use one NAI endpoint, one approved model, and one harmless Omegon tool.

1. Configure the NAI base URL and scoped credential.
2. Select `nai:<model-id>`.
3. Send a prompt requiring a deterministic read-only tool, such as reading a fixture file or returning a bounded environment status.
4. Observe a streamed assistant tool call.
5. Execute the tool locally under Omegon policy.
6. Return the correlated tool result to the same NAI model.
7. Receive a final streamed answer grounded in that result.
8. Confirm that logs identify the `nai` endpoint/model without exposing credentials or sensitive prompt content beyond configured telemetry policy.

This demonstrates the full post-gateway contract without conflating it with model deployment or Kubernetes operations.

## Acceptance criteria

The compatibility MVP is complete when all of the following are demonstrated against a real NAI Agent Gateway endpoint:

- Omegon selects a model using `nai:<model-id>` without masquerading as another provider.
- The gateway authenticates a scoped NAI credential over verified TLS.
- A normal streamed chat turn completes.
- Omegon publishes at least one valid tool schema accepted by the gateway/model.
- A streamed tool call is reconstructed with its name, ID, and JSON arguments intact.
- Omegon executes the approved local tool and sends the matching tool result.
- The model returns a final answer using that result.
- Invalid credentials produce an actionable authentication/policy error.
- An invalid model ID produces an actionable routing/model error.
- A gateway timeout or disconnect is distinguished from a local tool failure.
- No credential is written to logs, documents, manifests, or session exports.

## Compatibility test matrix

| Test | Required result |
|---|---|
| `GET /v1/models` | Record support or documented absence; not an MVP blocker |
| Non-streamed minimal chat probe | Successful response using configured model ID |
| Streamed chat | Ordered text deltas and clean completion |
| One function declaration | Request accepted without schema rejection |
| Tool call stream | Stable call ID, name, and parseable JSON arguments |
| Tool-result continuation | Final answer recognizes returned tool output |
| Unknown model | Clear gateway/model-routing diagnostic |
| Invalid credential | Clear `401`/`403` diagnostic without secret leakage |
| Rate limit | `429` classified as gateway throttling |
| Interrupted stream | Bounded retry/failure behavior; no duplicate tool execution |

## Security and governance constraints

- Gateway credentials should be scoped to the minimum endpoint/model permissions needed by the MVP.
- Local Omegon tools remain separately allowlisted and policy-controlled; model access does not imply tool authority.
- Tool calls are untrusted model output. Existing schema validation, path containment, process-spawn, and approval controls continue to apply.
- Retrying a turn after a partial stream must not replay a mutating tool call without idempotency or operator confirmation.
- Customer prompts, tool inputs, and tool results may contain sensitive data. Retention and telemetry settings must be agreed before production use.
- NAI gateway audit records and Omegon session/tool records should share correlation identifiers where supported, but their authorization domains remain separate.

## Explicit non-goals

- Installing or operating NKP, Kommander, NAI, KServe, vLLM, GPUs, or model artifacts.
- Replacing NAI model governance, endpoint RBAC, quotas, or gateway observability.
- Deploying LangChain or LangGraph as an inference platform.
- Reimplementing model serving inside Omegon.
- Production high availability, multi-tenant brokering, or autonomous mutating workflows.
- Assuming every OpenAI API feature is implemented by NAI.

## Risks and tradeoffs

### Named provider versus generic endpoint

A named `nai` provider is a small amount of provider-specific configuration, but it provides correct identity and safe defaults. A completely generic endpoint mechanism is architecturally cleaner and should follow through the inference manifest; making that broader runtime path a prerequisite would enlarge the MVP.

### Chat Completions versus Responses API

The current compatible path uses Chat Completions. If a future NAI release exposes only the OpenAI Responses API or gives it materially better tool semantics, Omegon will need an adapter selection rather than assuming `/v1/chat/completions` forever. That is not evidenced as an immediate MVP requirement.

### Gateway compatibility variance

OpenAI-compatible products often differ in accepted JSON Schema keywords, streaming tool-call fragments, optional request fields, and error bodies. The compatibility matrix is therefore an integration gate, not documentation polish.

## Open questions

- What exact NAI Agent Gateway release and endpoint product surface will the MVP target?
- What base path precedes `/v1/chat/completions` in the deployed environment?
- Does the gateway expose `GET /v1/models` to the intended credential?
- Which model is approved for the vertical slice, and does it support streamed tool calling?
- Which JSON Schema keywords does the gateway/model accept for tools?
- How are gateway credentials issued, rotated, scoped, and revoked?
- What enterprise CA, proxy, and `NO_PROXY` requirements apply from the Omegon runtime location?
- What prompt/tool-result retention is permitted for the customer environment?
- Does NAI expose a request ID that Omegon can preserve in session telemetry?
- Should the first runtime be an operator workstation, a bounded `omegon run` job, or long-running `omegon serve` inside NKP after compatibility is proven?

## Required tests

- Unit test that `nai` resolution requires both a base URL and credential and preserves provider identity.
- Unit test that endpoint normalization produces exactly one `/v1/chat/completions` suffix.
- Unit test that credential values cannot appear in normalized provider errors.
- Wire fixture for streamed text through an NAI-compatible response.
- Wire fixture for fragmented tool-call arguments and tool-result continuation.
- Negative fixtures for `401`, `403`, `404`/unknown model, `429`, malformed SSE, and interrupted streams.
- Live, opt-in conformance test against a real NAI endpoint, guarded by environment variables and excluded from normal offline test runs.

## Exit decision

If the vertical slice passes, Omegon is compatible enough to serve as the MVP agent runtime directly behind NAI Agent Gateway. The next design step is to promote arbitrary manifest-defined OpenAI-compatible endpoints into executable provider bridges, removing the need for a permanently special-cased NAI provider while retaining NAI endpoint identity and policy metadata.
