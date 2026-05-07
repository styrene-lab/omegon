+++
id = "be3d37b1-9865-4154-a277-c80fb42babcb"
kind = "document"
title = "OpenAI provider identity and routing honesty — separate API vs ChatGPT/Codex auth, route GPT models correctly, and surface the active engine truthfully"
status = "implemented"
tags = ["providers", "routing", "auth", "ux", "bugfix", "rust"]
aliases = ["openai-provider-identity-and-routing-honesty"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
branches = ["feature/openai-provider-identity-and-routing-honesty"]
issue_type = "bug"
open_questions = []
openspec_change = "openai-provider-identity-and-routing-honesty"
parent = "bridge-provider-routing"
priority = "1"
+++

# OpenAI provider identity and routing honesty — separate API vs ChatGPT/Codex auth, route GPT models correctly, and surface the active engine truthfully

## Overview

The Rust harness currently treats ChatGPT/Codex OAuth as if it authenticated the generic openai provider, causing the UI to advertise GPT-family models under openai:* even when only openai-codex credentials exist. At runtime, openai:* routes through OpenAIClient (API key + Chat Completions semantics) while the available credential may actually be an openai-codex JWT that only works through CodexClient. The design goal is to restore honesty at three layers: credential identity (OpenAI API vs ChatGPT/Codex OAuth), routing identity (GPT-family models choose the provider/client that can actually execute them), and operator visibility (the UI clearly shows which provider/model/credential path is active right now).

## Research

### Current mismatch in Rust auth and provider resolution

`auth.rs` defines provider id `openai` with `auth_key: "openai-codex"`, so the generic OpenAI surface reads ChatGPT/Codex OAuth credentials from the same auth.json entry used by `openai-codex`. `providers::resolve_api_key_sync("openai")` therefore reports OpenAI as authenticated when only ChatGPT OAuth exists. The model picker in `tui/mod.rs` uses that probe to offer `openai:gpt-5.4`, `openai:o3`, and related GPT-family models. But `resolve_provider("openai")` still constructs `OpenAIClient`, which calls `/v1/chat/completions` with API-key semantics; the separate `CodexClient` is the only Rust client that can use ChatGPT OAuth JWTs against `chatgpt.com/backend-api/codex/responses`.

### Existing routing and UX affordances already imply the needed split

`bridge-provider-routing` already established that provider identity must remain honest when `/model` changes, and `provider-neutral-model-controls` established that the operator must be able to see the concrete active provider/model, not just a tier label. The missing piece is provider-family honesty inside the OpenAI family: `openai` should mean API-key OpenAI unless a route explicitly targets the ChatGPT/Codex client, while GPT-family selection should still be able to land on `openai-codex` when that is the executable route. `unified-auth-surface` also expects auth status tables to show real backend identity and method, which this bug currently violates.

## Decisions

### Decision: `openai` auth means OpenAI API credentials only; ChatGPT OAuth remains `openai-codex`

**Status:** decided
**Rationale:** The harness must stop treating `openai-codex` storage as proof that the generic `openai` provider is executable. `resolve_api_key_sync("openai")`, auth status surfaces, and selector gating should report `openai` only when an actual OpenAI API key or equivalent OpenAI API credential exists. ChatGPT Plus/Pro OAuth stays a distinct provider identity (`openai-codex`) because it uses a different endpoint, auth artifact, and client implementation.

### Decision: GPT-family routing may fall through from `openai` intent to `openai-codex` execution when that is the only viable OpenAI-family path

**Status:** decided
**Rationale:** The operator’s model intent can stay GPT-family / OpenAI-family without lying about the executable backend. If a request targets an `openai:*` GPT-family model and no OpenAI API route is viable, the router may select `openai-codex` as the concrete provider when ChatGPT/Codex OAuth can actually execute the model. The resolved runtime surface must then report the concrete provider as `openai-codex` (or ChatGPT/Codex), not pretend the request ran on generic OpenAI API.

### Decision: All operator-facing auth and engine surfaces must show concrete provider identity and method inside the OpenAI family

**Status:** decided
**Rationale:** The auth status table, model selector gating, startup/bootstrap capability summary, and active engine display should distinguish at least: `OpenAI API` (API key) versus `ChatGPT/Codex` (OAuth). When a conversation is running, the operator should be able to see the resolved provider, resolved model, and credential class in the active engine surface so there is no ambiguity about whether the harness is using API OpenAI or ChatGPT/Codex.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `core/crates/omegon/src/auth.rs` (modified) — Separate auth identity/probing for OpenAI API vs ChatGPT/Codex OAuth in provider metadata and status surfaces
- `core/crates/omegon/src/providers.rs` (modified) — Align provider resolution, fallback, and OpenAI-family routing so openai credentials do not masquerade as openai-codex and GPT-family requests can resolve to the viable concrete client
- `core/crates/omegon/src/tui/mod.rs` (modified) — Update model selector and active engine/auth displays to distinguish OpenAI API from ChatGPT/Codex and surface the concrete active route
- `core/crates/omegon/src/main.rs` (modified) — Ensure CLI and startup/runtime surfaces report the resolved provider/model honestly
- `core/crates/omegon/src/routing.rs` (modified) — Reconcile default provider/model selection rules for OpenAI-family routes with the concrete provider split
- `core/crates/omegon/src/features/model_budget.rs` (modified) — Align tier/model copy and provider selection with OpenAI API vs openai-codex distinction
- `core/crates/omegon/src/providers.rs` (modified) — Add or expand Rust unit tests covering auth detection, provider inference, and GPT-family fallback behavior
- `core/crates/omegon/src/tui/mod.rs` (modified) — Add tests for selector gating or visible engine/auth labeling where feasible
- `core/crates/omegon/src/tui/footer.rs` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `core/crates/omegon/src/tui/tests.rs` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `core/crates/omegon/src/tui/snapshots/omegon__tui__snapshot_tests__snapshot_bootstrap_full.snap` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `core/crates/omegon/src/tui/snapshots/omegon__tui__snapshot_tests__snapshot_footer_default.snap` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `core/crates/omegon/src/tui/snapshots/omegon__tui__snapshot_tests__snapshot_footer_with_model_and_context.snap` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `core/crates/omegon/src/tui/snapshots/omegon__tui__snapshot_tests__snapshot_footer_with_persona_and_mcp.snap` (modified) — Post-assess reconciliation delta — touched during follow-up fixes

### Constraints

- The harness must never claim `openai` is authenticated solely because `openai-codex` OAuth credentials exist.
- Fallback from OpenAI-family intent to `openai-codex` execution must preserve honest reporting of the concrete provider actually used.
- Operator-visible engine/auth surfaces must show enough information to distinguish OpenAI API key execution from ChatGPT/Codex OAuth execution.
