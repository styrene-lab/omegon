+++
id = "9bd19927-891d-4492-a9f9-e32d254ea4b1"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Anthropic subscription automation boundary

Anthropic's consumer subscription is treated by Omegon as an **operator-risk** credential class for automation. Claude.ai / Anthropic subscription credentials are permitted for human-operated TUI sessions. For headless or unattended automation, Omegon warns clearly when subscription OAuth is the active Anthropic credential and then proceeds, preserving operator agency instead of silently changing providers or enforcing a hidden policy gate.

If you need scripted, headless, or background use, configure an Anthropic API key with `ANTHROPIC_API_KEY`.

## What Omegon allows and warns on

When Anthropic subscription auth is the only Anthropic credential available, Omegon treats entry points as follows:

| Entry point | Omegon behavior | Notes |
| --- | --- | --- |
| TUI mode | Allowed | Human-operated interactive sessions are the supported case. |
| `--initial-prompt` | Allowed | Seeds an interactive TUI session while keeping a human in the loop. |
| `--prompt` / `--prompt-file` | Warns and proceeds | Headless prompt execution is automation-risky with subscription OAuth. |
| `--smoke` | Warns and proceeds | Smoke runs are unattended checks. |
| `/cleave` | Routed to automation-safe fallback when possible; otherwise blocked | Omegon prefers OpenAI API → OpenAI/Codex OAuth → OpenRouter → Ollama before failing. |

That is the clean automation boundary:

- keep Anthropic subscription login for interactive sessions
- use `ANTHROPIC_API_KEY` for automation
- use another automation-safe provider when you want provider-policy-clean unattended execution

## Summary matrix

| Credential mode | Automation posture | Notes |
| --- | --- | --- |
| Anthropic API key | Unrestricted | Headless and automated use are allowed, subject to Anthropic API terms and limits. |
| Anthropic subscription / OAuth | Warns for automation | Fine for interactive TUI use; headless Anthropic execution emits an explicit warning and remains operator-owned. |
| OpenAI API key | Unrestricted | Standard API-key flow. |
| OpenAI/Codex OAuth | Unrestricted in Omegon | Separate provider path from OpenAI API; used for GPT-family/Codex-backed routing. |
| Ollama (Local) | Unrestricted | Local inference, no external account auth. |
| Ollama Cloud | Unrestricted | Hosted Ollama via `OLLAMA_API_KEY`. |

## Why Omegon is strict here

This is not a cosmetic warning. Provider terms are a runtime boundary.

Omegon's job is to keep the operator honest about which credential class is actually executing the work. For Anthropic subscription credentials, that means:

- no pretending a consumer subscription is equivalent to an API key
- no silent automation through a credential class Omegon has already classified as automation-risky
- explicit fallback to automation-safe providers when a workflow such as `/cleave` needs unattended execution
- explicit warnings when the operator chooses to run headless Anthropic work on subscription OAuth anyway

If you want the shortest interactive path, use the Anthropic subscription login.
If you want automation, use an API-key-backed provider.
