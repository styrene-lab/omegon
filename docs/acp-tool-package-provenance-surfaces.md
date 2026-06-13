---
id: acp-tool-package-provenance-surfaces
title: "ACP tool and package provenance surfaces"
status: deferred
tags: [acp, tools, packages, provenance, issue-132-followup]
open_questions:
  - "[assumption] Tool provenance can be captured at registration time without changing the public tool execution contract."
  - "[assumption] Package inventory should treat extensions, skills, plugins, agents, and catalog entries as package contributions, not separate UI silos."
dependencies:
  - acp-132-0-26-9-completion
related:
  - docs/unified-package-contribution-substrate.md
  - docs/acp-132-runtime-observability-extension-control.md
---

# ACP tool and package provenance surfaces

## Overview

This follow-up owns issue #132 surfaces that let ACP clients explain where capabilities came from: core runtime, bundled extension, installed extension, package, skill, plugin, or agent catalog contribution.

## Scope

- `_tools/list`
  - tool name/description/schema hash if useful
  - provider/provenance: core, extension, MCP, package, built-in
  - owning extension/package id when applicable
  - enabled/disabled/available state

- `_packages/list` refinement
  - installed package inventory
  - package contributions: tools, prompts, resources, skills, agents, extensions
  - install source, version, path, update/error state
  - relationship to `_packages/install`, remove, update, search

## Non-goals

- No extension RPC invocation; owned by 0.26.9 issue #132 completion.
- No package installation flow redesign unless required to expose accurate inventory.
- No permission diagnostics; owned by permissions surfaces.

## Design direction

Package inventory should become the UI-level capability explanation layer. Tool provenance should be derived from registration/contribution metadata, not inferred from naming conventions.

## Acceptance criteria

- ACP client can list all tools and determine which runtime/package/extension provided each tool.
- ACP client can list installed packages and the contributions each package materialized.
- Package/tool surfaces agree on package and extension IDs.

## Consolidation note

Active release work for this ACP topic has been consolidated into [[acp-0-27-closeout|ACP 0.27.0 closeout]]. This node remains as reference material for the closeout classification.
