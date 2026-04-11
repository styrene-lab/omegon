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

### Workspace is a coordination object, not a branch wrapper

A workspace is **not**:
- a branch
- a worktree
- a clone
- a jj checkout
- a pod
- a filesystem path label

Those are backing substrates.

A workspace **is** a runtime ownership and coordination boundary over a mutable plaintext surface.

That means the workspace abstraction is responsible for:
- mutability ownership
- workspace kind
- runtime role / authority
- local coordination state

It is **not** responsible for VCS topology.

### Workspace identity must remain separate from substrate identity

The model must keep distinct:
- `workspace_id` — machine/runtime coordination identity
- `label` — operator-facing human name
- `backend_kind` — how the surface is realized (`local-dir`, `git-worktree`, `git-clone`, `jj-checkout`, etc.)
- `vcs_ref` — optional descriptive VCS linkage (`git`/`jj`, branch/bookmark, remote, revision)

This separation is required so a workspace remains valid even if:
- branch names change
- history is rebased
- jj bookmarks move
- the same upstream project is mounted at different local paths
- the workspace has no VCS backing at all (for example a vault)

### Workspace binding should point into lifecycle, not collapse into it

Authority-sensitive workspaces should carry optional lifecycle bindings such as:
- `milestone_id`
- `design_node_id`
- `openspec_change`

These are **bindings**, not identities.

That means:
- a release workspace may be bound to milestone `0.15.10`
- a benchmark workspace may be bound to a benchmark-analysis node
- a feature workspace may be bound to a design node or spec change

But the workspace itself remains a runtime coordination object.

Lifecycle bindings answer **what this workspace is for**.
Workspace identity answers **which mutable surface this runtime owns**.

### VCS anchors should be descriptive reference, not primary identity

Git/jj commit or revision pointers are useful as:
- audit anchors
- reproducibility markers
- drift detectors

They should not become workspace identity.

A workspace may record a `vcs_ref` or future `vcs_anchor`, but authority should key primarily off role + lifecycle binding, with VCS state used secondarily for drift and provenance checks.
