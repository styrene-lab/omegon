+++
id = "82a74b13-97bd-43d2-a37b-808c84443ef9"
kind = "design_node"

[data]
title = "Skills Guided Creation and Installation"
status = "exploring"
issue_type = "design"
priority = 2
dependencies = []
open_questions = []
+++

# Skills Guided Creation and Installation

## Overview

Design how `/skills` and menu surfaces should guide operators through creating and installing skills without overlapping the existing extension installation paths.

The core distinction: **extensions install executable capability packages and tool surfaces**, while **skills are operator-editable instruction artifacts for smaller workflows where markdown plus optional helper scripts suffice**. `/skills` owns guided skill authoring, inspection, import, and in-place installation of plaintext skill files and adjacent scripts. Extension paths remain responsible for installing extension packages, providers, tools, bundled runtime capability surfaces, or anything that needs a first-class tool/API surface.

This node focuses on adding **in-place skill creation/import** rather than creating a parallel extension installer.

## Problem

Operators often have:

- Plaintext `SKILL.md` files from another project or machine.
- Small prompt/instruction snippets they want converted into a skill.
- Scripts or helper files they want placed alongside a skill.
- Bundled skills they want installed locally and then edited.

Today `/skills` exposes list/install/get/delete/create affordances, but guided creation/import is not clearly separated from `/extension`, `/plugin`, `/catalog`, and `/armory` flows. Without a clear boundary, `/skills install` can drift into duplicating extension installation semantics.

## Goals

- Make `/skills` the operator-facing surface for skill authoring, import, local installation, inspection, and deletion.
- Keep `/extension`, `/plugin`, `/catalog`, and `/armory` as package/capability installation paths.
- Support in-place creation from interactive guidance and existing plaintext files/directories containing `SKILL.md`.
- Preserve skills as editable project/user artifacts, not opaque packages.
- Expose the same actions through slash commands and menus.
- Validate skill metadata before installation where possible.

## Non-goals

- Do not implement a second extension installer under `/skills`.
- Do not make `/skills` install arbitrary executable extensions.
- Do not hide script execution behind skill import; scripts are files referenced by a skill, not new trusted tools by default.
- Do not require every imported skill to become a packaged extension.

## Command Surface

Existing commands to preserve:

- `/skills` or `/skills list` — palette inventory and actions.
- `/skills install` — install bundled skills into the user skill directory.
- `/skills install <name>` — install one bundled skill by name.
- `/skills get <name>` — inspect a skill.
- `/skills delete <name>` — delete a user/project-local skill.
- `/skills create` or `/skills new` — guided authoring prompt.

Additions covered by this change:

- `/skills create --project` — guided authoring with explicit project-local output intent.
- `/skills create --user` — guided authoring with explicit user-level output intent.
- `/skills import <path>` — guided import of an existing skill file or directory.
- `/skills import --project <path>` — guided import into `.omegon/skills/` for the current project.
- `/skills import --user <path>` — guided import into the user-level skills directory.

## Decisions

- Treat scoped create/import as runtime-prompt flows in the TUI, not direct control-runtime requests, until a structured import API exists.
- Escape path text embedded into generated import prompts so operator-provided paths cannot break markdown code spans.
- Keep `/skills install` scoped to bundled skills; do not overload it for arbitrary local files.

## Open Questions

- Should a future structured skill import API live in `control_runtime`, `skills`, or an extension-facing command registry?
- Should menu surfaces expose project/user destination as a selector before prompting?
