+++
id = "bb48ebe9-2ead-4896-9606-459ee77687de"
kind = "document"
title = "Login UX: company=api-key / product=subscription paradigm"
status = "seed"
tags = ["auth", "ux", "login", "oauth", "providers", "0.15.5"]
aliases = ["login-company-product-paradigm"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
dependencies = []
open_questions = []
parent = "unified-auth-surface"
related = []
+++

# Login UX: company=api-key / product=subscription paradigm

## Overview

Restructure /login so the command name unambiguously signals which auth surface the operator is targeting. Rule: company name → raw developer API key. Product/brand name → OAuth subscription flow. Eliminates the current confusion where 'anthropic' routes to OAuth and 'openai' routes to API key with no coherent principle.

Current broken state:
- `anthropic` id → OAuth (should be API key — Anthropic is the company)
- `openai` id → API key (correct)
- `openai-codex` id → OAuth (opaque hyphenated id, not a product name)
- `/login claude` alias exists but dispatches incorrectly

Target state:
- `/login anthropic` → ANTHROPIC_API_KEY (company = API)
- `/login claude` → OAuth Claude Pro/Max (product = subscription)
- `/login openai` → OPENAI_API_KEY (company = API)
- `/login codex` → OAuth ChatGPT/Codex subscription (product = subscription)
- `/login chatgpt` → alias for codex OAuth (more recognizable brand)

Pure-API providers (no subscription product distinction):
- openrouter, groq, xai, mistral, cerebras, huggingface → company name = API key only

Future slots reserved:
- `/login google` → Google API key; `/login gemini` → Google One/Gemini subscription
- `/login grok` → SuperGrok subscription (when xAI ships OAuth)

auth.rs changes required:
- Split current 'anthropic' (OAuth) into 'anthropic' (ApiKey) and 'claude' (OAuth)
- Rename 'openai-codex' to 'codex', keep auth_key='openai-codex' for storage compat
- Add 'chatgpt' alias dispatching to codex OAuth flow
- Update login selector grouping: Subscriptions (OAuth) above API Keys
- Update /login slash completions: add claude, codex, chatgpt; retain anthropic, openai

Backward compat: auth_key values in auth.json must not change (storage keys stay stable).
