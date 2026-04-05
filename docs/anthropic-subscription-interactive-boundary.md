---
id: anthropic-subscription-interactive-boundary
title: "Anthropic Subscription — Interactive vs Automated Use Boundary"
status: decided
tags: [anthropic, tos, auth, subscription, automated-use, compliance]
open_questions:
  - "Where exactly is the \\\\\\\\\\\\\\\\\"interactive\\\\\\\\\\\\\\\\\" line? Human types a prompt → agent runs 100 tool calls over 10 minutes unattended → human reads the result. Is that one interaction interactive or automated? The bot/script framing suggests the trigger matters more than the autonomy of the loop."
  - "What Omegon entry points fire against the Anthropic subscription token? Full inventory needed: TUI foreground, --background flag, daemon/service mode, cleave child workers, cron/scheduled invocations, CI runner invocations. Each needs a clear allow/warn/block decision."
  - "[assumption] A human watching the TUI while a long agentic task runs (many tool calls, minutes of autonomous work) counts as \\\\\\\\\\\\\\"interactive/non-automated\\\\\\\\\\\\\\" under Anthropic's ToS because the human initiated it and is present. This assumption needs validation — it's the most common Omegon use pattern and must be defensible."
  - "Should --initial-prompt (queues a prompt but TUI stays open, human is watching) be allowed with subscription credentials? The human is present but the trigger was scripted. Conservative read: allow it — TUI is open, human can intervene. Permissive read: allow it freely. No read blocks it — it's not --prompt (headless)."
dependencies: []
related: []
---

# Anthropic Subscription — Interactive vs Automated Use Boundary

## Overview

Anthropic's ToS for Claude.ai subscriptions (ANTHROPIC_OAUTH_TOKEN) restricts use to "non-automated" purposes. Omegon must clearly distinguish between interactive TUI use (a human is present, reading, and steering) and automated/background agent processes (unattended loops, daemon mode, background cleave workers). The former is defensible as "interactive"; the latter is a clear ToS violation. This node designs the enforcement and communication strategy: what the UX says, what gates we put in place, and where the line is drawn technically.

## Research

### Exact Anthropic Consumer ToS language (live, from anthropic.com/legal/consumer-terms)

The operative clause from Anthropic's Consumer Terms (governing Claude.ai / Claude Pro subscriptions — i.e. ANTHROPIC_OAUTH_TOKEN):

> "Except when you are accessing our Services via an Anthropic API Key or where we otherwise explicitly permit it, to access the Services through automated or non-human means, whether through a bot, script, or otherwise."

The exception carved out is explicit: if you have an Anthropic API Key, automation is fine. Subscriptions (OAuth token) are restricted to non-automated, human-present use.

The Commercial Terms of Service govern ANTHROPIC_API_KEY usage and explicitly do NOT restrict automation.

Note: Claude.ai individual and Claude Pro both fall under consumer terms. There is no enterprise subscription tier that gets automation rights without an API key (enterprise orgs that want automation must use the API billing path, not the subscription path).

### Omegon entry point inventory — automated vs interactive

Current entry points in main.rs:

INTERACTIVE (human present at terminal):
- TUI mode (no --prompt): Full TUI, human types and reads. Clear non-automated use.
- --initial-prompt: Pre-queues a prompt but TUI stays open, human is watching. Borderline — human present but prompt was scripted.

AUTOMATED (no human required, can be scripted/scheduled):
- --prompt "..." / --prompt-file: Headless execution, fire-and-forget. No TUI. Clearly automated.
- --smoke: Automated regression tests. Clearly automated.
- --smoke-cleave: Automated cleave tests. Clearly automated.
- Cleave child workers: Spawned as subprocesses, always use --prompt internally. Clearly automated.
- CI pipeline invocations: Any invocation without a TTY is automated by definition.

Key insight: the --prompt flag IS the automation gate. If --prompt is set, the process runs headless without human interaction. If not set, a TUI opens and a human is at the keyboard. These map cleanly to "automated" and "interactive" under Anthropic's ToS distinction.

## Decisions

### Hard-block headless/automated modes when ANTHROPIC_OAUTH_TOKEN is the only provider

**Status:** accepted

**Rationale:** The --prompt flag is already the idiomatic headless gate. Reusing it as the automation enforcement gate is consistent and costs no new complexity. Operators who hit the block get a clear, actionable message: upgrade to API key billing.

### Four-surface TUI disclosure for Anthropic subscription interactive-only constraint

**Status:** accepted

**Rationale:** Operators should know the constraint before they hit it, not after. The footer badge is always visible. The startup notice is once-only so it doesn't become noise. The /cleave guard is at the exact moment they try to do what's forbidden. The tutorial consent update closes the loop on the onboarding path.

### Credential priority and fallback rules under mixed-credential configurations

**Status:** accepted

**Rationale:** Operators often have both. The API key should always win for automated paths. This prevents the surprise case where someone adds an OAuth token for TUI use and unknowingly starts routing headless tasks through it.

### --initial-prompt is permitted with subscription credentials (TUI remains open, human is present)

**Status:** accepted

**Rationale:** Legal risk is negligible when a human is at the terminal. The test is whether a human is present and can intervene, not whether they initiated the first message by keystroke vs config.

### Operator-agency warning model for subscription ToS risk

**Status:** accepted

**Rationale:** Omegon warns clearly about Anthropic subscription automation risk and surfaces the active credential/mode, but does not fully remove the operator's ability to proceed. The harness should avoid silent fallback and hidden policy enforcement where possible; explicit warnings preserve operator agency while still performing due diligence.

### Hard-block headless/automated modes when ANTHROPIC_OAUTH_TOKEN is the only provider

**Status:** superseded

**Rationale:** Superseded by the accepted operator-agency warning model. Current implementation warns and proceeds rather than hard-blocking automated/headless Anthropic subscription use.

### Credential priority and fallback rules under mixed-credential configurations

**Status:** superseded

**Rationale:** The API-key-preferred portion remains useful, but the silent fallback/headless enforcement implication is superseded. Current cleave/runtime behavior avoids silent provider rerouting and keeps the requested model/provider explicit.

## Open Questions

- Where exactly is the \\\\\\\\\\"interactive\\\\\\\\\\" line? Human types a prompt → agent runs 100 tool calls over 10 minutes unattended → human reads the result. Is that one interaction interactive or automated? The bot/script framing suggests the trigger matters more than the autonomy of the loop.
- What Omegon entry points fire against the Anthropic subscription token? Full inventory needed: TUI foreground, --background flag, daemon/service mode, cleave child workers, cron/scheduled invocations, CI runner invocations. Each needs a clear allow/warn/block decision.
- [assumption] A human watching the TUI while a long agentic task runs (many tool calls, minutes of autonomous work) counts as \\\\\\\"interactive/non-automated\\\\\\\" under Anthropic's ToS because the human initiated it and is present. This assumption needs validation — it's the most common Omegon use pattern and must be defensible.
- Should --initial-prompt (queues a prompt but TUI stays open, human is watching) be allowed with subscription credentials? The human is present but the trigger was scripted. Conservative read: allow it — TUI is open, human can intervene. Permissive read: allow it freely. No read blocks it — it's not --prompt (headless).

## Implementation Notes

### Constraints

- Hard-block must happen before any network call to Anthropic — check at arg-parse time, not at first inference request
- Error message must cite the exact ToS URL so operator can verify
- The block applies to the credential, not the model — if --model openai:gpt-4o and ANTHROPIC_OAUTH_TOKEN is set, no block (different provider)
- Never silently fall back from ANTHROPIC_OAUTH_TOKEN to another provider in headless mode — that would hide the violation from the operator
