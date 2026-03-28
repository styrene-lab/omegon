---
id: rc1-openai-routing-honesty
title: "RC1: OpenAI-family routing honesty landing"
status: exploring
parent: release-0-15-4-trust-hardening
tags: [release, rc1, providers, auth, ux]
open_questions: []
dependencies: []
related:
  - openai-provider-identity-and-routing-honesty
---

# RC1: OpenAI-family routing honesty landing

## Overview

Release-checklist node for the second rc.1 acceptance criterion: OpenAI-family auth and routing honesty must be landed in a way the operator can actually trust. This includes distinguishing OpenAI API from ChatGPT/Codex OAuth in visible surfaces and ensuring that the concrete runtime provider/model shown to the operator matches the executable path used by the harness.

## Decisions

### Decision: rc.1 honesty requires bootstrap/auth summary, model selector gating, active engine display, and final reporting surfaces to distinguish OpenAI API from ChatGPT/Codex OAuth

**Status:** decided

**Rationale:** The operator does not trust a split that exists only internally. Rc.1 should require the minimum surface set that affects operator decisions and post-run understanding: the startup/bootstrap auth summary, model selector availability/gating, the active engine display during runtime, and final run/reporting surfaces. Footer and diagnostics can be included if already touched, but these four are the non-negotiable honesty surfaces for rc.1.

### Decision: rc.1 OpenAI-family proof cases are API-only, OAuth-only, both-present, and fallback from openai intent to openai-codex execution

**Status:** decided

**Rationale:** These four cases cover the real ambiguity boundary. Rc.1 should prove: (1) OpenAI API-only credentials present — `openai` executes honestly as API OpenAI, (2) ChatGPT/Codex OAuth-only credentials present — generic OpenAI API is not falsely shown as authenticated, but GPT-family intent can still resolve to `openai-codex` when appropriate, (3) both credential classes present — the chosen concrete provider is still reported honestly, and (4) a route that begins as `openai` intent but executes via `openai-codex` shows that fallthrough truthfully in the operator-visible surface.

### Decision: rc.1 OpenAI-family proof surfaces are the bootstrap provider lines, model selector options, engine panel provider/model/auth line, and post-run report evidence

**Status:** decided

**Rationale:** The code already gives us four concrete surfaces that matter and are testable. The bootstrap panel renders provider/auth state from `HarnessStatus` (`tui/bootstrap.rs`). The model selector builds gated choices based on separate OpenAI API and OpenAI Codex auth inputs (`tui/mod.rs::build_model_selector_options`, with existing tests in `tui/tests.rs`). The engine panel in the footer shows provider label, model, and auth class (`tui/footer.rs`). And the rc.1 routed-run/report evidence should carry the final concrete provider/model route. These surfaces are enough to prove the split honestly without waiting for a full dashboard redesign.

### Decision: OpenAI/Codex OAuth route has now passed a real repo-backed proof with honest route reporting

**Status:** decided

**Rationale:** A real repo-backed docs proof was executed using `openai-codex:gpt-5.4`. The child inherited `CHATGPT_OAUTH_TOKEN`, executed successfully, reported `openai-codex:gpt-5.4` as the concrete route, and completed end-to-end once the jj-native integration path was fixed. This converts the OpenAI/Codex OAuth route from a purely surface-level honesty claim into a validated execution path.
