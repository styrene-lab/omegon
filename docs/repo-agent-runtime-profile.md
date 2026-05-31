---
id: repo-agent-runtime-profile
title: "Repo Agent Runtime Profile"
status: exploring
tags: [agent-runtime, nex, devenv, subagents, a2a, workflow]
open_questions:
  - "[assumption] The repo profile should be declarative and versioned, with imperative setup delegated to Nex/devenv/devcontainer rather than embedded in the profile."
  - "Which file path should be canonical for the profile: `.omegon/profile.toml`, `.omegon/project.toml`, `.nex/agent-profile.toml`, or a neutral org-level path that external agents can discover?"
  - "How should external agents discover and consume the profile when they do not understand Omegon schemas: generated `AGENTS.md`, `CONTRIBUTING.md` sections, `.codex/config.toml`, `CLAUDE.md`, or profile explain output?"
  - "What is the minimal v1 schema surface: commands, validations, lifecycle mode, safety policy, secrets pack reference, substrate profile reference, delegation policy, or all of these?"
  - "How should profile trust be established for autonomous/headless agents: commit-tracked file only, signed profile hash, org policy allowlist, or A2A-delivered profile digest?"
dependencies: []
related:
  - deterministic-devenv-workflow-assessment
  - session-secret-cache-preflight
  - runtime-facade-command-event-model
---

# Repo Agent Runtime Profile

## Overview

Define a repo-local, versioned agent runtime profile that travels with the repository and tells Omegon, subagents, remote Omegon runtimes, and external coding agents how to operate safely and effectively in the codebase. The profile should compose with Nex/devenv/devcontainer substrate definitions rather than replacing them.

## Decisions

### Profile is repo contract; Nex provides substrate; Omegon enforces runtime policy

**Status:** candidate

**Rationale:** This separates concerns: the repository declares expected agent behavior and workflow; Nex answers whether the host can satisfy tools/secrets/environment; Omegon decides how agents act, delegate, validate, and request approval under that contract.

### Start read-only with inspect/explain/validate before behavior enforcement

**Status:** candidate

**Rationale:** A read-only first slice lets us stabilize schema and external-agent documentation without accidentally changing runtime behavior. Enforcement can follow once profile semantics and migration paths are clear.

## Open Questions

- [assumption] The repo profile should be declarative and versioned, with imperative setup delegated to Nex/devenv/devcontainer rather than embedded in the profile.
- Which file path should be canonical for the profile: `.omegon/profile.toml`, `.omegon/project.toml`, `.nex/agent-profile.toml`, or a neutral org-level path that external agents can discover?
- How should external agents discover and consume the profile when they do not understand Omegon schemas: generated `AGENTS.md`, `CONTRIBUTING.md` sections, `.codex/config.toml`, `CLAUDE.md`, or profile explain output?
- What is the minimal v1 schema surface: commands, validations, lifecycle mode, safety policy, secrets pack reference, substrate profile reference, delegation policy, or all of these?
- How should profile trust be established for autonomous/headless agents: commit-tracked file only, signed profile hash, org policy allowlist, or A2A-delivered profile digest?
