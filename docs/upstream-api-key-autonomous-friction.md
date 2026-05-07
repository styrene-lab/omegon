+++
id = "a5451fe9-e6fd-48b9-bbb9-0624071f28c2"
kind = "document"
title = "Upstream API-key autonomous mode — friction points and mitigations"
status = "implemented"
tags = ["autonomy", "providers", "api-keys", "routing", "engineering-note"]
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Upstream API-key autonomous mode — friction points and mitigations

This note ranks the main friction points for running Omegon in autonomous mode with an upstream API key, grounded in current code and docs.

## Summary

The biggest problem is not basic API-key support. Omegon already treats direct API keys as the clean path for unattended execution. The real friction is that the autonomous path is **asymmetric across providers and entry points**:

1. Anthropic automation policy is enforced unevenly
2. "Automation-safe" fallback is narrower than the broader provider surface
3. Provider fallback during execution is intentionally conservative, which increases surprise when the requested upstream is unavailable
4. Tutorial and interactive affordances bias toward cloud keys that Omegon explicitly classifies as automation-safe, so autonomous operators with other credentials can see inconsistent guidance
5. Autonomous mode still depends on operator understanding of provider naming and routing behavior

The system is already pointing in the right direction: API keys are preferred over consumer OAuth, and the docs are explicit that API keys are the sanctioned route for automation. The mitigation work is mostly about tightening policy consistency and reducing operator surprise.

## Ranked friction points

### 1. Anthropic automation policy is warning-only in CLI automation, but hard policy elsewhere

**Why this is friction**

Omegon documents Anthropic subscription/OAuth as interactive-only and says headless prompt execution is hard-blocked when that is the only Anthropic credential. But the CLI path shown in code currently emits a warning and proceeds.

That creates an operator hazard: autonomous users can infer that Anthropic credential policy is consistently enforced, when in fact one path still relies on operator judgment.

**Evidence**

- `core/crates/omegon/src/main.rs:979-1003` defines `anthropic_subscription_automation_warning(cli)` and returns a warning string rather than blocking execution for `--smoke`, `--smoke-cleave`, `--prompt`, and `--prompt-file`.
- The warning text explicitly says: "Omegon is proceeding because operator agency wins" and recommends `ANTHROPIC_API_KEY` for unrestricted automation (`core/crates/omegon/src/main.rs:996-1000`).
- `docs/anthropic-subscription-tos.md` says the opposite policy at the product-doc level: `--prompt` / `--prompt-file` and `--smoke` are "Hard-blocked," while Anthropic API keys are "Unrestricted" (`docs/anthropic-subscription-tos.md`).

**Impact on autonomous API-key users**

Even if the operator has a valid upstream API key available for some providers, the Anthropic policy mismatch makes the autonomy surface feel unreliable. People running unattended jobs need predictable enforcement, not a mix of docs-level prohibition and code-level warnings.

**Mitigation direction**

- Unify the policy boundary: either hard-block these Anthropic OAuth automation paths in `main.rs`, or downgrade the doc language so it matches reality.
- Prefer the former. The current docs are the better design: interactive-only OAuth, API key for automation.
- Surface the exact remediation in the error path: "configure `ANTHROPIC_API_KEY` or switch to an automation-safe provider."

---

### 2. Automation-safe fallback is narrower than the supported provider matrix

**Why this is friction**

Omegon supports many providers, but the helper that picks an "automation-safe" model only prefers a small subset. That is sensible as a safety posture, but it means autonomous users with valid upstream API keys for other supported providers may not get the behavior they expect.

**Evidence**

- `core/crates/omegon/src/providers.rs:52-68` documents `automation_safe_model()`.
- That helper currently prioritizes only:
  1. OpenAI API key
  2. OpenRouter
  3. Ollama local
  (`core/crates/omegon/src/providers.rs:55-58`, `68-76`).
- By contrast, the canonical provider map lists a much broader supported matrix: OpenAI, OpenRouter, Groq, xAI, Mistral, Cerebras, HuggingFace, Ollama, Ollama Cloud, plus Anthropic and OpenAI Codex under their respective auth modes (`docs/provider-credential-map.md`).

**Impact on autonomous API-key users**

An operator with only `GROQ_API_KEY`, `MISTRAL_API_KEY`, or `CEREBRAS_API_KEY` may reasonably expect Omegon's autonomy helpers to recognize those as first-class automation-safe routes. Current code does not do that in the default helper.

**Mitigation direction**

- Expand `automation_safe_model()` to cover the broader direct API-key providers already supported by the runtime.
- Keep the safety rule explicit: include direct API-key providers, exclude consumer OAuth routes.
- If product wants a narrower default set, rename the helper to reflect that it is a **preferred default subset**, not the full automation-safe universe.

---

### 3. Execution fallback is conservative and provider-local, which increases surprise

**Why this is friction**

Execution fallback does not behave like a general "pick any working provider" router. It stays mostly within the requested provider family. That reduces accidental cross-provider drift, but it means autonomous runs fail sooner than operators may expect.

**Evidence**

- `core/crates/omegon/src/providers.rs:314-335` defines `fallback_order_for_model(model_spec)`.
- Anthropic requests only fall back to `anthropic` (`core/crates/omegon/src/providers.rs:325`).
- Groq, xAI, Mistral, Cerebras, HuggingFace, Ollama, and Ollama Cloud each only fall back to themselves (`core/crates/omegon/src/providers.rs:326-333`).
- Only the OpenAI/OpenAI-Codex family has cross-provider fallback, and only within that family (`core/crates/omegon/src/providers.rs:317-324`).
- Meanwhile `docs/provider-credential-map.md` describes a broader fallback chain in prose, starting from Anthropic and moving through OpenAI, OpenAI Codex, Groq, xAI, Mistral, HuggingFace, Cerebras, OpenRouter, and Ollama.

**Impact on autonomous API-key users**

This is an expectation trap. The docs imply a broad fallback posture, but execution resolution is stricter. An unattended run targeting a missing or misconfigured provider can fail even when another valid upstream API key is available.

**Mitigation direction**

- Decide whether execution fallback should remain provider-local or adopt the broader documented chain.
- If provider-local is intentional, document it clearly as a constraint of autonomous execution.
- If the broader chain is desired, update `fallback_order_for_model()` to match the documented routing policy.

---

### 4. Tutorial and onboarding gates are optimized for a subset of automation-safe routes

**Why this is friction**

The interactive tutorial is not the autonomous runtime, but it is part of the operator's mental model. Today it treats only some routes as the "clear happy path" for AutoPrompt, which can make capable autonomous configurations feel second-class.

**Evidence**

- `core/crates/omegon/src/tui/tutorial.rs:46-62` defines `tutorial_gate()`.
- It marks the tutorial `Interactive` when an OpenAI API key or OpenRouter key is present (`core/crates/omegon/src/tui/tutorial.rs:47-52`).
- Anthropic API key also becomes `Interactive`, but Anthropic OAuth becomes `ConsentRequired` and no Anthropic credential becomes `OrientationOnly` (`core/crates/omegon/src/tui/tutorial.rs:58-61`).
- The tutorial note describes direct API-key routes as the explicit happy path and treats consumer OAuth as non-default for automation (`docs/free-tier-tutorial.md`, section "Tutorial variant matrix by capability tier").

**Impact on autonomous API-key users**

An operator with a supported but non-highlighted upstream route can see inconsistent signals: the runtime supports the provider, but the onboarding story emphasizes other routes. That increases time-to-confidence when setting up headless usage.

**Mitigation direction**

- Align onboarding language with the actual autonomous provider surface.
- If only a subset is officially recommended, say that directly: "supported" versus "recommended for autonomous mode."
- Reuse the same provider classification source for docs, tutorial gating, and runtime fallback.

---

### 5. Provider naming and credential-class semantics are easy to misread

**Why this is friction**

Omegon distinguishes between provider families, auth classes, and wire protocols. That is the right architecture, but it creates setup friction for autonomous users who expect one provider label to imply one operational behavior.

**Evidence**

- `docs/provider-credential-map.md` explicitly separates:
  - Anthropic API key / OAuth under `anthropic`
  - OpenAI API key under `openai`
  - OpenAI Codex / ChatGPT OAuth under `openai-codex`
- The same doc also notes that OpenAI Codex / ChatGPT OAuth is experimental and not a first-class supported backend for general use (`docs/provider-credential-map.md`, "LLM Providers — Proprietary Protocol").
- `core/crates/omegon/src/tui/tutorial.rs:55-57` reflects this by keeping ChatGPT/Codex consumer OAuth out of the automation-safe default path.

**Impact on autonomous API-key users**

The operator must understand more than "I have an OpenAI-related credential" or "I logged into a consumer product." For autonomous mode, the exact credential class matters. That is correct but still friction.

**Mitigation direction**

- Expose credential class more aggressively in UX and docs: API key, OAuth, dynamic, local.
- Show the resolved execution provider and why it was chosen before autonomous runs start.
- When a credential is excluded from automation-safe routing, say whether the reason is policy, capability, or support maturity.

## What is already working

These are not speculative; current code already does the important architectural part correctly:

- Anthropic API key wins over Anthropic OAuth in credential resolution (`core/crates/omegon/src/providers.rs:40-49`).
- Tutorial gating treats direct API-key-style routes as the safe default for live automation-like steps (`core/crates/omegon/src/tui/tutorial.rs:46-62`).
- Product docs explicitly tell the operator to use `ANTHROPIC_API_KEY` for automation and reserve consumer subscription credentials for interactive use (`docs/anthropic-subscription-tos.md`).

That means the main gap is not missing architecture. It is **policy consistency across code paths and operator-facing surfaces**.

## Recommended mitigation plan

### Short term

1. **Make Anthropic automation enforcement consistent**
   - Update `core/crates/omegon/src/main.rs:979-1003` to hard-block the same cases the docs already classify as blocked.
2. **Clarify fallback semantics**
   - Either update `docs/provider-credential-map.md` to distinguish "global strategic fallback" from "per-request execution fallback," or expand code fallback to match docs.
3. **Document the autonomous happy path explicitly**
   - Add a concise matrix: supported for autonomy, recommended for autonomy, interactive-only.

### Medium term

1. **Broaden `automation_safe_model()`**
   - Include supported API-key providers beyond OpenAI and OpenRouter where quality/support is acceptable.
2. **Centralize provider policy classification**
   - One shared source should drive tutorial gating, CLI autonomy checks, and fallback selection.
3. **Add preflight transparency for autonomous runs**
   - Print resolved provider, credential class, and fallback reason before headless execution begins.

### Long term

1. **Separate "supported" from "recommended" routing tiers**
   - The current code is mixing those ideas.
2. **Codify policy tests around auth class and autonomy**
   - Especially for Anthropic OAuth, OpenAI Codex OAuth, and automation-safe selection.

## Bottom line

Omegon is already opinionated in the correct direction: **upstream API keys are the right primitive for autonomy**. The main friction is that the implementation and docs do not yet express that rule with one voice across all entry points and routing helpers.

If this gets cleaned up, the autonomy story becomes much simpler:

- API key: unattended execution is allowed
- Consumer OAuth: interactive-only or explicitly gated
- Fallback: predictable and documented
- Tutorial/onboarding: consistent with runtime policy
