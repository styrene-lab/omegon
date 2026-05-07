+++
id = "9bd19927-891d-4492-a9f9-e32d254ea4b1"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Anthropic subscription ToS compliance

Anthropic's consumer subscription is treated by Omegon as an **interactive-only** credential class. Claude.ai / Anthropic subscription credentials are permitted for human-operated TUI sessions, but Omegon now **hard-blocks** headless and unattended automation paths when that is the only Anthropic credential available.

If you need scripted, headless, or background use, configure an Anthropic API key with `ANTHROPIC_API_KEY`.

## What Omegon allows and blocks

When Anthropic subscription auth is the only Anthropic credential available, Omegon treats entry points as follows:

| Entry point | Omegon behavior | Notes |
| --- | --- | --- |
| TUI mode | Allowed | Human-operated interactive sessions are the supported case. |
| `--initial-prompt` | Allowed | Seeds an interactive TUI session while keeping a human in the loop. |
| `--prompt` / `--prompt-file` | Hard-blocked | Headless prompt execution is treated as automation. |
| `--smoke` | Hard-blocked | Smoke runs are unattended checks. |
| `/cleave` | Routed to automation-safe fallback when possible; otherwise blocked | Omegon prefers OpenAI API → OpenAI/Codex OAuth → OpenRouter → Ollama before failing. |

That is the clean automation boundary:

- keep Anthropic subscription login for interactive sessions
- use `ANTHROPIC_API_KEY` for automation
- use another automation-safe provider when Anthropic subscription is the only interactive credential on the machine

## Summary matrix

| Credential mode | Automation posture | Notes |
| --- | --- | --- |
| Anthropic API key | Unrestricted | Headless and automated use are allowed, subject to Anthropic API terms and limits. |
| Anthropic subscription / OAuth | Interactive only | Fine for interactive TUI use; Omegon blocks automated/headless Anthropic execution. |
| OpenAI API key | Unrestricted | Standard API-key flow. |
| OpenAI/Codex OAuth | Unrestricted in Omegon | Separate provider path from OpenAI API; used for GPT-family/Codex-backed routing. |
| Ollama (Local) | Unrestricted | Local inference, no external account auth. |
| Ollama Cloud | Unrestricted | Hosted Ollama via `OLLAMA_API_KEY`. |

## Why Omegon is strict here

This is not a cosmetic warning. Provider terms are a runtime boundary.

Omegon's job is to keep the operator honest about which credential class is actually executing the work. For Anthropic subscription credentials, that means:

- no pretending a consumer subscription is equivalent to an API key
- no silent automation through a credential class Omegon has already classified as interactive-only
- explicit fallback to automation-safe providers when a workflow such as `/cleave` needs unattended execution

If you want the shortest interactive path, use the Anthropic subscription login.
If you want automation, use an API-key-backed provider.