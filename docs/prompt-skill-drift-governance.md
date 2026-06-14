+++
title = "Prompt and skill drift governance"
tags = ["prompts","skills","lifecycle","automation","capabilities","extensions"]
+++

+++
id = "d0fc7de3-9b16-4e14-a712-840e42c0f06f"
kind = "design_node"

[data]
title = "Prompt and skill drift governance"
status = "exploring"
issue_type = "epic"
priority = 1
dependencies = []
open_questions = []
+++

## Overview

# Prompt and skill drift governance

---
title: Prompt and skill drift governance
status: exploring
tags: [prompts, skills, lifecycle, automation, capabilities, extensions]
date: 2026-06-14
---

# Prompt and skill drift governance

## Overview

Omegon's model-facing guidance has drifted away from the harness internals it describes. The highest-risk surfaces are the no-extension prompt templates (`prompts/*.md`), dynamic system-prompt injections (`core/crates/omegon/src/prompt.rs`, `core/crates/omegon/src/context.rs`), and bundled skills (`skills/*/SKILL.md`, embedded through `core/crates/omegon/src/skills.rs`).

The immediate objective is to bring prompts and skills back into lockstep with the modern capability-driven harness: tool-group gating, `manage_tools`, canonical `edit`/`validate`, Workbench/plan state, Flynt document/task/design surfaces, and extension-provided capabilities.

This node intentionally targets prompt/skill maturity first. A second pass should assess bundled skill consolidation, including whether language/tooling skills should move into an `omegon-coding` extension that enables or disables guidance based on the operator profile and detected project shape.

## Evidence from initial audit

- `prompts/init.md` and `prompts/status.md` reference `memory_query` and direct `design_tree` actions even when those tools are not always in the exposed tool surface.
- `core/crates/omegon/src/prompt.rs` injects lifecycle guidance that names `design_tree_update`, `openspec_manage`, and slash-command workflows without first establishing that those capabilities are exposed in the current mode.
- `core/crates/omegon/src/context.rs` has hardcoded tool-group guidance for stale or mode-sensitive tools including `memory_query`, `memory_connect`, `design_tree_update`, and `openspec_manage`.
- `skills/code-act/SKILL.md` can override canonical small-edit behavior by telling the agent to prefer Python scripts over sequential tool calls whenever active.
- `skills/oci/SKILL.md` says `Containerfile` is canonical, while `prompts/init.md` only probes `Dockerfile`.
- `core/crates/omegon/src/skills.rs` embeds ten bundled skills by `include_str!`, so source drift and installed `~/.omegon/skills` drift are both possible.

## Scope

### In scope — first pass

- Audit no-extension prompt templates in `prompts/*.md`.
- Audit harness-level prompt assembly and dynamic injections in:
  - `core/crates/omegon/src/prompt.rs`
  - `core/crates/omegon/src/context.rs`
- Define and implement an automated drift check that catches references to unavailable, hidden, renamed, or deprecated model-facing tools.
- Update prompt/skill guidance so it describes capabilities rather than assuming static tool names.
- Add tests or validation commands that fail when prompt guidance drifts from the registered tool/capability inventory.

### In scope — second pass

- Audit bundled skills in `skills/*/SKILL.md` for accuracy, overlap, and activation behavior.
- Decide whether language-specific skills (`rust`, `python`, `typescript`) remain bundled global skills or move into an `omegon-coding` extension/profile package.
- Evaluate consolidation of cross-cutting skills (`git`, `security`, `code-act`, `style`, `vault`, `oci`, `openspec`) into profile-aware bundles.

### Out of scope for this node

- Rewriting the entire extension system.
- Changing Lex Imperialis semantics.
- Removing lifecycle tools or Flynt surfaces.
- Solving prompt-template execution safety beyond stale capability references, except where `/loop` or repeated prompt execution directly affects drift checks.

## Candidate design direction

1. **Capability inventory as source of truth.** Generate a machine-readable inventory of model-facing tools, hidden/group-gated tools, internal-only tools, prompt templates, bundled skills, and extension-provided skills.
2. **Prompt linting.** Add a lint that scans prompt templates, bundled skills, and prompt assembly string literals for tool-like references and classifies them as valid, gated, internal-only, deprecated, or unknown.
3. **Mode-aware language.** Replace unconditional instructions such as “use `design_tree`” with capability-aware language: “if lifecycle tools are exposed, use them; otherwise use `manage_tools` or continue with local docs.”
4. **Bundled skill lifecycle.** Treat bundled skills as versioned harness artifacts with validation, changelog requirements, and install/update drift checks.
5. **Profile-aware consolidation.** Consider an `omegon-coding` extension that owns language-specific and coding-loop skills, enabling them based on operator profile, project detection, and active tool surface.

## Open questions

- [assumption] The current registry can provide enough metadata to distinguish model-facing, hidden/group-gated, extension-provided, and internal-only tools for prompt linting.
- [assumption] Prompt references to slash commands should be linted differently from direct tool calls because some slash commands are operator controls, not model tools.
- Should prompt templates be executable instructions, operator-facing recipes, or both? The lint rules differ depending on provenance.
- What is the canonical location for prompt/skill drift automation: Rust unit tests, a `just` validation target, `scripts/`, or a new harness self-check command?
- Should `omegon skills install` detect and warn about installed bundled skills older than the embedded source?
- Should language-specific skills be always-available, project-detected, operator-profile-selected, or extension-owned?
- What assumptions is this design making that haven't been stated?

## Initial implementation notes

Likely files:

- `prompts/init.md`
- `prompts/status.md`
- `prompts/new-repo.md`
- `prompts/oci-login.md`
- `skills/code-act/SKILL.md`
- `skills/oci/SKILL.md`
- `skills/openspec/SKILL.md`
- `skills/rust/SKILL.md`
- `skills/python/SKILL.md`
- `skills/typescript/SKILL.md`
- `core/crates/omegon/src/prompt.rs`
- `core/crates/omegon/src/context.rs`
- `core/crates/omegon/src/skills.rs`
- `CHANGELOG.md`

Validation should include prompt rendering in slim/full modes and a static scan over prompt/skill markdown.

## Open Questions
