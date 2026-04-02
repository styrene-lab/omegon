---
id: skill-location-architecture
title: "Skill location standard and bundled skill distribution"
status: exploring
tags: [skills, architecture, distribution, dx]
open_questions:
  - "What is the lookup mechanism for bundled skills? Options: (a) a `~/.config/omegon/skills/index.json` manifest generated at install time, (b) filesystem glob at startup, (c) skills compiled into the binary as `include_str!`. Which wins on startup cost vs. updateability?"
  - "How does the harness load skills into context? Are all active skills injected into the system prompt at session start, or loaded on-demand? What is the size budget constraint?"
  - "[assumption] The existing `skills/` SKILL.md files are worth preserving as bundled skills. Are any of them stale enough to delete rather than migrate (same question as the cleave skill)?"
  - "Does the plugin system (armory.rs / registry.rs) need to change to support the three-tier load order (project-local → bundled → default), or is this purely a path-resolution change at the loader level?"
  - "What is the install-time mechanism for copying `skills/` to `~/.config/omegon/skills/`? Is this a `just install` step, a post-install hook in the brew formula, or both?"
dependencies: []
related: []
---

# Skill location standard and bundled skill distribution

## Overview

Skills are markdown directive files that inject domain knowledge into the harness at session start. Currently they live in `skills/` inside the Omegon source tree — inaccessible to any project that doesn't have a clone of the Omegon repo open.

The desired architecture has three tiers:

1. **Project-local** — `.omegon/skills/<name>/SKILL.md` in any repo using Omegon. Gittracked, project-specific. Loaded automatically when that project is open.

2. **Bundled** — skills that ship with Omegon, installed to `~/.config/omegon/skills/` during `just install` / brew / other distribution. These come from `skills/` in this upstream repo. Available to any project without a source clone.

3. **Lookup index** — a manifest that maps skill names to their bundled locations, so the harness can discover available skills without filesystem globbing across arbitrary directories.

The current `skills/` directory in this repo becomes the canonical upstream skill library — source of truth for bundled skills. Installation copies them to `~/.config/omegon/skills/`.

Existing skills to migrate/evaluate: git, rust, typescript, python, openspec, oci, style, security, vault, pi-extensions, pi-tui.

## Open Questions

- What is the lookup mechanism for bundled skills? Options: (a) a `~/.config/omegon/skills/index.json` manifest generated at install time, (b) filesystem glob at startup, (c) skills compiled into the binary as `include_str!`. Which wins on startup cost vs. updateability?
- How does the harness load skills into context? Are all active skills injected into the system prompt at session start, or loaded on-demand? What is the size budget constraint?
- [assumption] The existing `skills/` SKILL.md files are worth preserving as bundled skills. Are any of them stale enough to delete rather than migrate (same question as the cleave skill)?
- Does the plugin system (armory.rs / registry.rs) need to change to support the three-tier load order (project-local → bundled → default), or is this purely a path-resolution change at the loader level?
- What is the install-time mechanism for copying `skills/` to `~/.config/omegon/skills/`? Is this a `just install` step, a post-install hook in the brew formula, or both?
