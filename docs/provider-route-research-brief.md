# Research Brief — Subscription-First Provider Route Matrix

Produce a **route matrix**, not a provider overview.

The goal is to answer:

> For a given model family, what concrete execution routes are available to an operator, through which entitlement/auth mechanism, with what capability, cost, and policy tradeoffs?

## Core instruction

Each row must represent **one concrete execution route**, not just a vendor or model brand.

You must clearly separate:

- **operator-facing entitlement** — what the human thinks they bought
- **auth mechanism** — how Omegon gets credentials
- **execution backend** — what Omegon would actually call
- **model family** — Claude, GPT, Gemini, OSS/local, etc.

Do **not** collapse these into one “provider” label.

---

## Required output schema

Return structured rows in JSON or markdown table form matching this schema as closely as possible:

```json
{
  "model_family": "Gemini",
  "route_id": "gemini-api-key",
  "operator_entitlement": "Gemini API billing",
  "entitlement_type": "api_billing",
  "auth_mechanism": "api_key",
  "credential_artifacts": ["GEMINI_API_KEY", "GOOGLE_API_KEY"],
  "execution_backend": "Gemini Developer API",
  "base_url": "https://...",
  "protocol_shape": "native|openai-compatible|custom-oauth|local-daemon",
  "official_support_level": "official|documented-compatible|community-only|unclear",
  "models_reachable": ["gemini-2.5-pro", "gemini-2.5-flash"],
  "tool_calling": "yes|no|unknown",
  "streaming": "yes|no|unknown",
  "multimodal_input": "yes|no|unknown",
  "web_search_or_grounding": "native|external-tool-only|none|unknown",
  "context_window_notes": "text",
  "rate_limit_or_quota_surface": "headers|status-endpoint|none|unknown",
  "automation_posture": "allowed|warning|blocked|unknown",
  "terms_risk": "low|medium|high|unknown",
  "cost_shape": "subscription|per-token|local|hybrid|unknown",
  "operator_value_case": "why an operator would prefer this route",
  "evidence": [
    {
      "type": "official_doc|official_pricing|official_terms|official_example|community_report",
      "url": "https://...",
      "claim": "what this source supports"
    }
  ],
  "confidence": "high|medium|low",
  "open_questions": [
    "what remains unclear"
  ]
}
```

---

## What to collect for each route

### 1. Route identity
You must provide:

- `model_family`
- `route_id`
- `execution_backend`

Examples of good route ids:

- `anthropic-api-key`
- `claude-consumer-oauth`
- `openai-api-key`
- `chatgpt-codex-oauth`
- `gemini-api-key`
- `google-ai-pro-oauth` *(only if a real programmatic route exists)*
- `ollama-local`
- `ollama-cloud-api`

Do **not** give only vendor names like “Google” or “Ollama”.

---

### 2. Entitlement vs backend split
For each route, distinguish:

#### Operator-facing entitlement
Examples:
- Claude Pro/Max
- ChatGPT Plus/Pro
- Google AI Pro
- Gemini API billing
- OpenAI API billing
- Ollama Cloud account
- local hardware/runtime

#### Concrete backend
Examples:
- Anthropic API
- Claude consumer endpoint
- Codex backend
- Gemini API
- Ollama local daemon
- Ollama Cloud API

This distinction is mandatory.

---

### 3. Auth artifact specifics
Collect exact credential artifacts where possible.

Examples:
- `ANTHROPIC_API_KEY`
- `ANTHROPIC_OAUTH_TOKEN`
- `CHATGPT_OAUTH_TOKEN`
- `GEMINI_API_KEY`
- `GOOGLE_API_KEY`
- `OLLAMA_API_KEY`
- `OLLAMA_HOST`

Also note:
- whether OAuth is browser/device-based
- whether token refresh is needed
- whether auth depends on a CLI session
- whether auth comes from a billing project

---

### 4. Capability surface per route
Report route-level capabilities, not vague family-level claims.

At minimum:
- tool calling
- streaming
- multimodal input
- web search / grounding / fetch
- context window notes
- rate-limit / quota telemetry surface

If unknown, say `unknown`.

---

### 5. Automation / policy posture
You must classify each route as one of:

- `allowed`
- `warning`
- `blocked`
- `unknown`

And explain why.

This should be based on:
- official terms
- official docs
- support statements
- absence of evidence where relevant

Do not present speculation as fact.

---

### 6. Evidence quality
Every major claim should cite evidence tagged as one of:

- `official_doc`
- `official_pricing`
- `official_terms`
- `official_example`
- `community_report`

Prefer official sources.
Use community evidence only when official docs are silent, and mark confidence accordingly.

---

### 7. Cost / operator value
For each route, include:
- `cost_shape`
- `operator_value_case`

Examples:
- “Best if the operator already pays for ChatGPT Pro and wants coding access without separate API billing.”
- “Best for sanctioned automation and clear API terms.”
- “Best for local/private usage on owned hardware.”

---

## Minimum routes to research first

### Claude family
- Anthropic API key
- Claude subscription / consumer OAuth

### GPT family
- OpenAI API key
- ChatGPT/Codex OAuth

### Gemini family
- Gemini API key
- Google AI Pro subscription path *(only if a real programmatic route exists)*
- Vertex/Google Cloud Gemini route if materially distinct

### OSS / local family
- Ollama local
- Ollama Cloud

### Optional second wave
- OpenRouter
- Groq
- Hugging Face
- Mistral
- Cerebras

---

## Questions you must answer explicitly

For each route, identify unresolved questions including:

1. Is this route **officially supported for programmatic use**?
2. Is automation **explicitly allowed**, merely tolerated, or unclear?
3. Does the route expose **telemetry/quota data**?
4. Does the route have **tool-calling parity** with API-key paths?
5. Is the route **stable enough to be a happy path**?

---

## What not to send back

Do **not** send:

- a vendor marketing summary
- one prose paragraph per provider with no structure
- model-family claims without route identity
- “Google AI Pro is cost-effective” without proving a programmatic route
- “Ollama Cloud exists” without auth/API details

---

## Final instruction

Produce a **route matrix**, not a provider catalog.

Each row must represent one concrete execution route to a model family, including:
- operator entitlement
- auth mechanism
- backend identity
- capability surface
- automation posture
- cost shape
- evidence links

Separate **what the operator bought** from **what Omegon would actually call**.

If a subscription-backed route is unclear, say so explicitly rather than smoothing it over.
