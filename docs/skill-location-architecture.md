+++
id = "4071287e-61c3-4c66-83b5-f85562f4ab7b"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Skill location standard and bundled skill distribution

## Overview

Skills are markdown directive files that inject domain knowledge into the harness at session start. Currently they live in `skills/` inside the Omegon source tree — inaccessible to any project that doesn't have a clone of the Omegon repo open.

The desired architecture has three tiers:

1. **Project-local** — `.omegon/skills/<name>/SKILL.md` in any repo using Omegon. Gittracked, project-specific. Loaded automatically when that project is open.

2. **Bundled** — skills that ship with Omegon, installed to `~/.config/omegon/skills/` during `just install` / brew / other distribution. These come from `skills/` in this upstream repo. Available to any project without a source clone.

3. **Lookup index** — a manifest that maps skill names to their bundled locations, so the harness can discover available skills without filesystem globbing across arbitrary directories.

The current `skills/` directory in this repo becomes the canonical upstream skill library — source of truth for bundled skills. Installation copies them to `~/.config/omegon/skills/`.

Existing skills to migrate/evaluate: git, rust, typescript, python, openspec, oci, style, security, vault, pi-extensions, pi-tui.

## Decisions

### Bundled skills install to ~/.omegon/skills/, consistent with harness convention

**Status:** accepted

**Rationale:** ~/.omegon/ is already the user-level harness home (same pattern as other harnesses using this convention). No special index or manifest needed — just a standard subdirectory. The harness discovers skills via filesystem at two locations: ~/.omegon/skills/ (bundled/user) and .omegon/skills/ (project-local). Project-local wins on name collision.

### Skills inject into system prompt at session start via registry.build_system_prompt()

**Status:** accepted

**Rationale:** Consistent with Lex Imperialis philosophy: capabilities are always-present identity, not on-demand reads. The registry already assembles the system prompt; skills are an additional layer appended after Lex/Tone/Persona. Load order: glob ~/.omegon/skills/*/SKILL.md first, then .omegon/skills/*/SKILL.md (project-local wins on name collision by overwriting).

### Install step: `just install` copies skills/ to ~/.omegon/skills/

**Status:** accepted

**Rationale:** Simple rsync/cp in the Justfile install recipe. Brew formula post-install hook does the same. No manifest or index file needed — the directory structure is the index.

### Existing skills audit: review remaining 10 skills before migrating

**Status:** deferred

**Rationale:** Each remaining skill (git, rust, typescript, python, openspec, oci, style, security, vault, pi-extensions, pi-tui) needs review — same question as cleave: is the content still accurate or stale? Deferred to implementation phase so migration doesn't copy junk to ~/.omegon/skills/.

### Plugin system: add skill loading to PluginRegistry, not armory TOML system

**Status:** accepted

**Rationale:** The armory system requires plugin.toml manifests and a separate omegon-armory repo — overkill for simple markdown directives. Skills are just files: glob ~/.omegon/skills/*/SKILL.md and .omegon/skills/*/SKILL.md, load content, append to system prompt. PluginRegistry gets a loaded_skills: Vec<String> field. The armory's existing skill plugin type remains available for richer plugin authors who want manifests.

## Open Questions

- What is the lookup mechanism for bundled skills? Options: (a) a `~/.config/omegon/skills/index.json` manifest generated at install time, (b) filesystem glob at startup, (c) skills compiled into the binary as `include_str!`. Which wins on startup cost vs. updateability?
- How does the harness load skills into context? Are all active skills injected into the system prompt at session start, or loaded on-demand? What is the size budget constraint?
- [assumption] The existing `skills/` SKILL.md files are worth preserving as bundled skills. Are any of them stale enough to delete rather than migrate (same question as the cleave skill)?
- Does the plugin system (armory.rs / registry.rs) need to change to support the three-tier load order (project-local → bundled → default), or is this purely a path-resolution change at the loader level?
- What is the install-time mechanism for copying `skills/` to `~/.config/omegon/skills/`? Is this a `just install` step, a post-install hook in the brew formula, or both?
