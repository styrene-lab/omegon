+++
id = "3ace7acb-c974-412a-ae0e-3fb8543b96d4"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Site/docs polish for 0.15.10 — reconcile public docs with current product reality

## Overview

Audit the public site and repo documentation for missing, stale, or misleading content around the 0.15.10 release line. Focus on release-facing install/get-started/provider/tutorial/TUI/docs pages, identify drift against the shipped product, and produce a prioritized set of concrete doc/site fixes.

## Research

### Initial drift audit

Initial audit found multiple public-doc drift candidates. install.astro still uses hardcoded 0.15.7 in VERSION/cosign examples and claims 'Claude Pro/Max Subscription (Free)' even though that path depends on a paid upstream subscription (`site/src/pages/docs/install.astro:19-21`, `:49-56`, `:63-65`). quickstart.astro still describes the old right-side instrument panel + terse footer instead of the unified footer console/instrument/dashboard reality (`site/src/pages/docs/quickstart.astro:18-20`, `:35-40`). providers.astro says `/login openai` for ChatGPT/Codex OAuth, but current docs and design work distinguish OpenAI API auth from ChatGPT/Codex auth, so that wording likely needs tightening (`site/src/pages/docs/providers.astro:18-20`, `:70-80`). tutorial.astro says the tutorial web-dashboard step tells operators to run `/dash`, but changelog says the tutorial now auto-opens the dashboard during that step (`site/src/pages/docs/tutorial.astro:24`, `CHANGELOG.md:310`). README still describes the old split-panel footer and outdated `.omegon-version` example `0.14.1`, so repo-root docs are behind current 0.15.10-era UI/runtime reality (`README.md:32-38`, `:122-130`).

### Tutorial drift is structural, not cosmetic

The tutorial docs are not just stale text; they no longer describe the actual runtime behavior. The shipped overlay is compiled from `core/crates/omegon/src/tui/tutorial.rs` and supports two distinct modes (`TutorialChoice::Demo` and `TutorialChoice::MyProject`) with adaptive steps, AutoPrompt phases, a command-triggered `/cleave` step in demo mode, and an auto-opened web dashboard step (`STEP_WEB_DASHBOARD`). The command handler in `core/crates/omegon/src/tui/mod.rs:960-1040` still supports `/tutorial`, `/tutorial demo`, `/tutorial lessons`, `/tutorial status`, `/tutorial reset`, plus `/next` and `/prev` for legacy lessons. The key handler in `core/crates/omegon/src/tui/mod.rs:4955-5032` confirms that leaving the 'Web Dashboard' step emits `TuiCommand::StartWebDashboard` automatically; the site docs incorrectly tell the operator that `/dash` is the tutorial step. This means the tutorial page needs a full rewrite around current modes, triggers, and outcomes rather than line edits.

## Decisions

### Public docs should center only the current overlay tutorial flows

**Status:** decided

**Rationale:** The shipped tutorial is an overlay with two primary operator-facing modes: the adaptive current-project flow (`/tutorial`) and the scripted showcase flow (`/tutorial demo`). Legacy lesson-file commands still exist in runtime for compatibility, but documenting `/tutorial lessons`, `/next`, and `/prev` as first-class onboarding surfaces makes the docs misleading. Public docs should describe the current tutorial product, not historical scaffolding.

## Open Questions

- Which public-facing pages should be treated as release-gated truth for 0.15.10: README + install + quickstart + providers + commands + tutorial + TUI, or a broader set including migration/get-started pages?
- [assumption] The command surface documented on the site still matches the shipped TUI slash-command registry; this should be verified before editing the command reference.
