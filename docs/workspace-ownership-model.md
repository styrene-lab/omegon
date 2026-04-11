---
id: workspace-ownership-model
title: "Workspace ownership model — one mutable agent per workspace"
status: exploring
tags: []
open_questions:
  - "What local runtime artifact should be the source of truth for mutable workspace ownership: per-workspace lease file only, a project-level local registry, or both?"
  - "How should a second mutable agent attach behave by default: refuse, offer read-only attach, or auto-create a sibling worktree/workspace?"
  - "How are release and benchmark authority isolated so RC cuts and release-candidate benchmarks cannot silently target post-tag HEAD state?"
  - "How should workspace kind be inferred vs explicitly declared so Omegon supports code repos, Obsidian vaults, spec repositories, and generic plaintext workspaces without assuming 'directory with files' means 'code project'?"
  - "What is the minimum sovereignty contract for workspace backends so local filesystem, bare git, self-hosted Forgejo/Gitea/GitLab, GitHub Enterprise, and Azure DevOps all behave as equivalent git transports rather than product-specific workspace types?"
dependencies: []
related: []
---

# Workspace ownership model — one mutable agent per workspace

## Overview

Omegon currently has a strong model for **project state**:
- the project is git-bound
- durable cognition is tracked in git (`.omegon/`, docs, specs, memory facts)
- lifecycle state is designed to survive across sessions and machines

What it lacks is an equally strong model for **workspace state**.

That gap shows up whenever multiple agents operate in parallel against the same repository path:
- RC identity becomes ambiguous
- benchmark provenance becomes untrustworthy
- controller tuning loses causal attribution
- multiple mutable agents can silently share a filesystem like they are one engineer, which they are not

The correct mental model is simple:

> Parallel Omegon agents should behave like parallel engineers.

That means parallel mutable work must be isolated in separate workspaces, just as two engineers would work on separate branches/worktrees.

This design introduces a first-class **workspace ownership** model so the filesystem hygiene problem is solved at the runtime/control-plane layer rather than left to operator folklore.

A second requirement is equally important:

> Omegon must not confuse “directory with files” or even “git repository” with “code project.”

Versioned plaintext workspaces are first-class. Obsidian vaults, spec repositories, design/documentation repos, and mixed plaintext+code workspaces are all legitimate Omegon projects.

A third requirement follows from sovereignty:

> Omegon should depend on git semantics, not forge brand names.

Local filesystem, bare git, self-hosted Forgejo/Gitea/GitLab, GitHub Enterprise, and Azure DevOps should all be treated as equivalent git-backed transports from the workspace model’s perspective.

## Decisions

### Workspace ownership is a first-class runtime primitive

**Status:** decided

**Rationale:** Parallel mutable Omegon agents must behave like parallel engineers. Project state remains durable and git-tracked, but workspace ownership, leases, and occupancy are machine-local runtime coordination state. One mutable agent per workspace becomes the core filesystem hygiene rule, and cleave uses the same workspace model as all other parallel execution.

### Workspace kinds are first-class and git transport is forge-neutral

**Status:** decided

**Rationale:** Omegon must not assume a filesystem tree or git repository is a code project. Versioned plaintext workspaces such as Obsidian vaults are first-class. Workspace kind should be inferred heuristically but operator-declarable, and git sovereignty means Forgejo/Gitea/GitLab/GitHub/Azure DevOps/local bare git are transport variants, not different workspace ontologies.
