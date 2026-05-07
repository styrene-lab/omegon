+++
id = "b612a524-8dc9-4310-8bac-536b5230c776"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# OpenAI provider identity and routing honesty — Tasks

## 1. Auth and provider resolution core
<!-- specs: providers/openai-family-routing -->

- [x] 1.1 Update `core/crates/omegon/src/auth.rs` so `openai` probing reflects only OpenAI API credentials and `openai-codex` remains the ChatGPT/Codex OAuth identity.
- [x] 1.2 Update `core/crates/omegon/src/providers.rs` so OpenAI-family resolution never treats `openai-codex` OAuth as proof that `openai` is executable.
- [x] 1.3 Implement OpenAI-family fallback so GPT-family intent can resolve to `openai-codex` when no OpenAI API route is viable, while preserving honest reporting of the concrete provider.
- [x] 1.4 Add or update unit tests in `core/crates/omegon/src/providers.rs` and related auth tests covering auth detection, provider inference, and GPT-family fallback behavior.

## 2. Routing policy and runtime reporting
<!-- specs: providers/openai-family-routing -->

- [x] 2.1 Update `core/crates/omegon/src/routing.rs` so OpenAI-family default model/provider selection is consistent with the concrete provider split.
- [x] 2.2 Update `core/crates/omegon/src/features/model_budget.rs` so effort-tier copy and provider/model resolution distinguish OpenAI API from `openai-codex`.
- [x] 2.3 Update `core/crates/omegon/src/main.rs` and any startup/runtime reporting surfaces needed to describe the resolved provider/model honestly.

## 3. TUI surface honesty
<!-- specs: providers/openai-family-routing -->

- [x] 3.1 Update `core/crates/omegon/src/tui/mod.rs` model selector gating so OpenAI API models are not advertised solely because ChatGPT/Codex OAuth exists.
- [x] 3.2 Update active engine/auth/provider displays in `core/crates/omegon/src/tui/mod.rs` and `core/crates/omegon/src/tui/footer.rs` to distinguish `OpenAI API` from `ChatGPT/Codex OAuth` and show the concrete active route.
- [x] 3.3 Add or update TUI-facing tests covering selector gating and visible engine/auth labeling where feasible.

## 4. Verification

- [x] 4.1 Run targeted Rust tests for auth, provider routing, and TUI surfaces.
- [x] 4.2 Run equivalent required Rust checks for touched surfaces (`cargo check -p omegon`; no TypeScript surfaces were changed).
- [x] 4.3 Reconcile `tasks.md` to reflect merged implementation reality before `/assess spec openai-provider-identity-and-routing-honesty`.
