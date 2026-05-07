+++
id = "dc06254d-65ee-4a23-b204-d5605c5debb3"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Lifecycle Artifact Versioning Policy

## Overview

Bake in a repository policy that lifecycle artifacts used as durable design and implementation documentation are version controlled by default, while preserving selective omission for transient cleave workspace/runtime artifacts.

## Research

### Why this needs automation instead of convention-only

Leaving `docs/` or `openspec/` files untracked creates a misleading repository state: lifecycle artifacts appear to exist in the working tree but are absent from history, which breaks their role as durable human-readable documentation. The most reliable enforcement point is the standard repository validation path (`npm run check`) so drift is caught before commit.

## Decisions

### Decision: Durable lifecycle artifacts are tracked; transient cleave workspaces stay optional

**Status:** decided
**Rationale:** Anything under durable project lifecycle paths such as `docs/` and `openspec/` should be version controlled because those files are part of the human project record. Cleave runtime workspaces and worktrees remain transient because they are machine-local execution artifacts created outside the repository and can be regenerated from the durable lifecycle sources.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `CONTRIBUTING.md` (modified) — Document lifecycle artifact version-control policy and the durable/transient split
- `package.json` (modified) — Run lifecycle-artifact tracking validation as part of the standard check script
- `extensions/openspec/lifecycle-files.ts` (new) — Implement helpers that detect untracked durable lifecycle artifacts via git status
- `extensions/openspec/lifecycle-files.test.ts` (new) — Test lifecycle artifact classification and untracked detection messaging

### Constraints

- Design-tree and OpenSpec files inside the repository are durable artifacts and should fail standard validation if left untracked
- Transient cleave workspaces outside the repository remain optional and must not be pulled into the durability check
- Validation messaging should explicitly tell contributors to git add durable lifecycle files or move transient artifacts outside docs/ and openspec/
