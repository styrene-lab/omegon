+++
id = "057e6453-995d-47ab-81fa-f168ad12fa70"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Research Brief — Subscription Route Verification Dossier

Replace the earlier broad provider matrix with a focused verification pass on the disputed subscription-backed and hosted routes.

The goal is to answer:

> Which non-API-key routes are real enough, official enough, and capable enough to become first-class happy paths for operators who already pay for model access?

This is **not** a vendor overview. It is a verification dossier for the routes where entitlement, auth mechanism, and runtime backend are currently ambiguous.

---

## Primary routes to investigate

Research these exact routes first:

1. `google-ai-pro`
2. `ollama-cloud`
3. `chatgpt-codex-oauth`
4. `claude-pro-oauth`

Optionally include comparison rows for the clear API-key equivalents only where needed to clarify the gap:

- `gemini-api-key`
- `openai-api-key`
- `anthropic-api-key`
- `ollama-local`

The point is not to restate the API-key paths. The point is to determine whether the **subscription-backed / hosted alternatives** can be treated as supported operator-first happy paths.

---

## Required output format

Produce one dossier section per route using this exact structure.

```markdown
## Route: <route_id>

### Verdict
- Happy-path candidate: yes|no|not yet
- Confidence: high|medium|low
- Recommended classification: happy-path | supported-with-caveats | experimental | operator-owned-risk | do-not-support

### Operator-facing entitlement
- What the human believes they bought

### Concrete execution backend
- What Omegon would actually call
- Exact host/base URL if known
- Whether this is distinct from the vendor's API billing route

### Auth mechanism
- Exact auth type: api_key | oauth | session token | local daemon | bearer token | unknown
- Exact credential artifacts / env vars / CLI dependencies
- Whether refresh is required

### Official support status
- official | documented-compatible | community-only | unclear
- Explain why, with citations

### Automation / terms posture
- allowed | restricted | prohibited | unclear
- Must cite exact terms/docs if possible
- Distinguish policy from Omegon preference

### Capability surface
- tool calling: yes|no|unknown
- streaming: yes|no|unknown
- multimodal input: yes|no|unknown
- search / grounding / fetch: native|external-only|none|unknown
- telemetry / quota surface: headers | status endpoint | none | unknown
- context window notes

### Technical stability
- high | medium | low
- Why: protocol churn, session fragility, anti-bot measures, official SDK, etc.

### Operator value case
- Why an operator would prefer this route over API billing

### Key evidence
- List of official docs, pricing pages, terms pages, forum statements, or community evidence
- Each item should be tagged as:
  - official_doc
  - official_pricing
  - official_terms
  - official_example
  - community_report

### Open questions
- What remains unresolved after research
```

---

## What to prove for each route

For each disputed route, answer these questions explicitly:

1. **Is there a sanctioned programmatic path at all?**
   - Not “can someone scrape it?”
   - I need to know whether there is an official or at least documented-compatible way to use this route programmatically.

2. **What exact backend would Omegon talk to?**
   - Consumer web backend?
   - Dedicated coding backend?
   - Hosted API?
   - OpenAI-compatible surface?
   - Unknown?

3. **What exact auth artifact exists?**
   - OAuth token?
   - session cookie?
   - bearer token?
   - API key?
   - dynamic CLI session?

4. **What is the automation posture?**
   - Explicitly allowed?
   - Restricted to interactive use?
   - Prohibited?
   - Unclear?

5. **What capability gap exists vs API billing?**
   - Tool calling
   - streaming
   - multimodal
   - search/grounding
   - telemetry
   - context window

6. **Is it stable enough to be a happy path?**
   - Or is it an operator-owned risk route?

---

## Route-specific guidance

### 1. `google-ai-pro`
This is the highest priority.

You must determine:
- whether Google AI Pro / Gemini Advanced has any sanctioned programmatic route
- whether it maps to Gemini API in any official way
- whether Google account auth can be used for developer access
- whether the entitlement is app-only rather than API-capable

The key output here is a clear answer to:

> Is Google AI Pro merely an entitlement concept, or does it map to a usable developer backend?

If the answer is “no verified programmatic path,” say that clearly.

### 2. `ollama-cloud`
You must determine:
- exact host/base URL
- exact auth artifact (`OLLAMA_API_KEY` or otherwise)
- whether it is an official Ollama-hosted service or just third-party hosting guidance
- whether it supports native Ollama API and/or OpenAI-compatible API
- whether tool calling and web search/web fetch are available in cloud mode
- whether telemetry/quota surfaces exist

The key output here is:

> Is Ollama Cloud a distinct hosted execution backend that should be modeled separately from `ollama-local`?

### 3. `chatgpt-codex-oauth`
Do not describe this vaguely as “browser scraping” unless that is the only defensible technical description.

You must determine:
- what exact backend surface is involved
- whether there is any official or documented-compatible route
- whether this is meaningfully distinct from GPT Actions and from OpenAI API billing
- what evidence supports classifying it as community-only, unsupported, or tolerated

**Current verification outcome:** treat as operator-owned-risk / internal experimental only. Do not present it as a happy path or a sanctioned subscription bridge.

The key output here is:

> What is the real technical and support classification of the Codex/ChatGPT OAuth route Omegon is already exposing?

### 4. `claude-pro-oauth`
You must determine:
- whether any sanctioned developer route exists for Claude Pro / Team / Max subscriptions
- whether consumer OAuth is app-only
- what the exact automation terms posture is
- whether this should be classified as do-not-support vs operator-owned-risk

**Current verification outcome:** treat as supported-with-caveats via the Claude Code OAuth/CLI path, with explicit throttling/backoff and no claim of unrestricted automation capacity.

The key output here is:

> Is there any route here beyond consumer interactive use?

---

## What not to send back

Do **not** send:

- a generic provider comparison
- marketing summaries
- vague statements like “X is cost-effective” without backend evidence
- route descriptions without exact auth/backend details
- conclusions without links to evidence

---

## Final instruction

Produce a **verification dossier** for the disputed subscription-backed and hosted routes.

The purpose is to decide whether each route should be classified as:
- first-class happy path
- supported with caveats
- experimental
- operator-owned risk
- do not support

Separate:
- what the operator bought
- how Omegon would authenticate
- what backend Omegon would execute against
- what the terms and capability boundaries actually are

If a route is unresolved, say so explicitly. Do not smooth over ambiguity.
