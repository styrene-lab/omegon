---
id: free-tier-tutorial
title: Free-tier tutorial — onboarding without a Pro subscription or API key
status: exploring
parent: tutorial-system
dependencies: [startup-systems-check]
tags: [tutorial, free-tier, accessibility, local-inference, onboarding, 0.15.1]
open_questions: []
jj_change_id: kvywttuknzuoxmkzorsyqmsrwqvwpnku
priority: 2
---

# Free-tier tutorial — onboarding without a Pro subscription or API key

## Overview

Design a tutorial experience for users who have no paid API subscription at all. Three user segments exist: (1) beefy machine — can run full local stack, treat like a normal user but with local models, (2) normal laptop — can maybe run a small model, limited capabilities, (3) no GPU at all — needs a completely different approach. The question: what can we show someone with zero tokens to spend that still demonstrates why Omegon matters?

## Research

### User segments and their capabilities

Three segments exist for users with no paid API key:

**Segment A: Beefy local machine** (32GB+ RAM, Apple Silicon M2 Pro+ or NVIDIA GPU with 12GB+ VRAM)
- Can run 14B-32B models locally via Ollama
- Full agent loop possible — local model as driver
- Tutorial experience: identical to paid, but slower and dumber
- Key gap: local models are worse at multi-step tool use, so the demo's 4-tool-call auto-prompts may produce worse results

**Segment B: Normal laptop** (16GB RAM, Apple Silicon M1/M2 or integrated GPU)
- Can run 4B-8B models — enough for delegation, memory extraction, compaction
- NOT enough for driver — these models can't reliably do multi-step tool orchestration
- Tutorial experience: needs a cloud driver (even free tier) for the agent loop, local for support tasks
- Key gap: Claude free tier has aggressive rate limits (messages per day)

**Segment C: Minimal hardware** (8GB RAM, no GPU, or older Intel Mac)
- Can maybe run a 1B-4B model — barely useful
- Tutorial experience: needs cloud entirely, or a fundamentally different demo
- Key gap: everything costs tokens they don't have

The Anthropic free tier (claude.ai) gives ~15-20 messages per day. That's enough for maybe 3-4 tutorial auto-prompt steps if we're careful. The current demo has 4 auto-prompts. It's tight but feasible if we don't waste messages.

### Tutorial design options for zero-cost onboarding

**Option 1: UI-only tour (zero tokens)**
Skip all AutoPrompt steps. The tutorial becomes a pure cockpit tour — Tab through 5-6 steps that explain the panels, show pre-rendered screenshots or canned output of what the AI would do, explain the workflow. No agent turns fire at all. This shows off the UI and explains the concepts but doesn't demonstrate real AI work.

Pro: Works for everyone. Zero cost. Fast.
Con: Doesn't show the magic. The user sees a cockpit they've never seen fly.

**Option 2: Minimal demo (2-3 agent turns)**
Strip the demo to its essentials: (1) read the project, (2) explain the fix plan, (3) done. Skip the design decision step, skip the cleave, skip the verify. The user sees the AI read code and explain it — that's impressive enough for a taste. Then say "upgrade for the full parallel fix experience."

Pro: Shows real AI work. 2-3 messages is within free tier limits.
Con: Doesn't show the signature feature (parallel branches).

**Option 3: Local-powered demo (zero cloud cost)**
For Segment A users: run the entire demo with a local model as driver. The results will be rougher but the experience is complete — read, design, spec, cleave, verify, browser. The tutorial adapts its expectations: "local models may produce different output, that's fine."

Pro: Full experience, zero cost. 
Con: Only works for beefy machines. Local models may fail at complex tool orchestration.

**Option 4: Hybrid — local driver + cloud fallback**
Try local first. If the local model fails a step (can't do multi-tool orchestration), fall back to one cloud API call for that step. Budget: max 2-3 cloud calls, rest local. The tutorial adapts based on what's available.

Pro: Best of both worlds.
Con: Complex routing logic. Hard to test. User experience unpredictable.

**Option 5: Pre-recorded demo**
Ship a recorded session as a replay — the tutorial "plays back" a real session with real tool calls and real output, but no live inference. The user sees exactly what a real session looks like, with real timing, real instrument activity, real design tree updates. Like watching a gameplay trailer before buying the game.

Pro: Perfect quality. Zero cost. Works everywhere.
Con: Not live. The user isn't doing anything. Less impressive once they realize it's canned.

### Free API tier landscape (March 2026)

**Anthropic API free tier**: $5 free credits on signup with phone verification. At Haiku 4.5 pricing ($1/$5 per 1M tokens), that's ~1M input tokens + 200K output tokens — enough for roughly 20-30 agent turns. More than enough for a full tutorial. Rate limit: 5 RPM on free tier, which is fine for a tutorial (one turn at a time). No free tier without credits purchase — but $5 gets you in. The claude.ai web free tier is NOT programmatic API access.

**Groq free tier**: No credit card required. Llama 3.3 70B at 394 TPS. Limits: ~14,400 requests/day, 6,000-18,000 TPM depending on model. This is generous for a tutorial. Groq exposes an OpenAI-compatible API. Key advantage: zero cost, fast inference, open models. Key disadvantage: not Anthropic — tool use quality may be lower with Llama vs Claude.

**Google Gemini free tier**: No credit card required. Gemini 2.5 Flash at 15 RPM free. 1M token context window. OpenAI-compatible endpoint. Rate limits cut ~50-80% in Dec 2025 but still usable for a tutorial. Free access to frontier-adjacent models.

**Together.ai**: Free $1 credits on signup. Open models (Llama, Mistral). OpenAI-compatible API.

**Summary**: A user with ZERO dollars can get meaningful API access through Groq (completely free, fast, Llama 70B) or Gemini (free, Google models). Both expose OpenAI-compatible APIs that Omegon can route to. The tutorial doesn't need Anthropic to work — it needs a model that can do tool calls.

### Tutorial variant matrix by capability tier

The systems check produces a capability tier. Each tier gets a tutorial variant:

**Tier 1: Full cloud** (Anthropic or OpenAI API key present)
→ Current STEPS_DEMO / STEPS_HANDS_ON. No changes needed. Omegon sacrifices wallet, full experience.

**Tier 2: Beefy local** (Ollama running, 14B+ model loaded or pullable, 32GB+ RAM)
→ STEPS_DEMO with local driver. Same steps, same prompts. Auto-prompts fire against local model. Results rougher but complete. Tutorial step text adapts: "The AI is running locally on your machine — no API costs." If a step fails (local model can't orchestrate tools), show a recovery prompt: "This step needs a more capable model. Skip with Tab, or add a cloud API key with /login."

**Tier 3: Groq free cloud** (No API key, no beefy local, but internet access)
→ Guide the user through Groq signup (30 seconds, no credit card) or Gemini API key setup in the tutorial's first step. Then run STEPS_DEMO with Groq as provider. Step text adapts: "Running on Groq's free tier — Llama 70B at zero cost."

**Tier 4: Small local** (Ollama with 4B-8B model, 16GB RAM)
→ Abbreviated tutorial. Skip cleave (small models can't orchestrate parallel branches). Show: read code, store facts, create a design node. 3-4 steps. Text: "Your machine can run a small AI model. Here's what Omegon looks like — upgrade to a larger model or add a cloud API for the full experience."

**Tier 5: Nothing** (No API key, no Ollama, no GPU)
→ Option 2 from the research: UI-only cockpit tour with pre-rendered explanations. Zero agent turns. Text: "You're exploring the cockpit. To see the AI in action, run Ollama locally (free) or sign up for Groq's free API (30 seconds, no credit card)." Provide actionable next steps, not a dead end.

The key principle: **never show a user a blank screen with no path forward.** Every tier has something to do, something to see, and a clear upgrade path.

### Groq routing already works via OPENAI_BASE_URL

Omegon's OpenAIClient already reads `OPENAI_BASE_URL` env var (defaults to `https://api.openai.com`). Setting `OPENAI_BASE_URL=https://api.groq.com/openai` with a Groq API key routes all OpenAI-provider traffic through Groq's Llama models.

This means the free-tier tutorial path is:
1. Systems check detects no Anthropic key, no OpenAI key
2. Tutorial step 0 says: "No cloud API detected. Get one for free in 30 seconds:"
3. Shows two options: "Groq (free, no credit card)" / "I'll add my own key with /login"
4. If Groq chosen: guide user through console.groq.com → copy API key → paste into /login
5. Set OPENAI_BASE_URL automatically and route through OpenAI client

The guided Groq signup could be a Command-trigger tutorial step: "Paste your Groq API key below." The tutorial waits for the user to type `/login openai <key>`, then sets the base URL and continues.

No new provider implementation needed. The infrastructure exists.

## Decisions

### Decision: Groq free tier as the zero-cost cloud fallback for tutorials

**Status:** exploring
**Rationale:** Groq offers a completely free API with no credit card, running Llama 3.3 70B at 394 tokens/sec with ~14K requests/day. It's OpenAI-compatible, so Omegon can route to it without a new provider implementation. The rate limits are more than sufficient for a tutorial (one turn at a time). Llama 70B is capable enough for tool-use orchestration. The tradeoff: it's not Claude-quality, so complex multi-tool steps may produce rougher results. But for a first impression? Watching AI read code at 400 tokens/sec for free is more impressive than not seeing it at all.

### Decision: Auto-detect capabilities at startup, offer override in the tutorial choice widget

**Status:** exploring
**Rationale:** The systems check (startup-systems-check) knows: API keys present, Ollama availability, GPU/RAM profile. This is enough to auto-select the right tutorial variant. The project-choice widget (already exists in tutorial.rs step 0) can present the detected option as the default with an override: "We detected [Ollama with 14B model / Groq free tier / Claude API]. Starting demo with [local inference / free cloud / full cloud]. Press ← → to change." Asking "do you have a Pro subscription?" is a mood killer. Telling them "we found Ollama running, let's use that" is empowering.

## Open Questions

*No open questions.*
