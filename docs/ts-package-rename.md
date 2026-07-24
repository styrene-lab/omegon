+++
id = "3709cc9e-ca3e-4570-b5b9-3c4b16da8aa4"
kind = "document"
title = "Rename TS Omegon npm package from `omegon` to `omegon-pi`"
status = "implemented"
tags = ["npm", "breaking-change", "distribution"]
aliases = ["ts-package-rename"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
+++

# Rename TS Omegon npm package from `omegon` to `omegon-pi`

## Overview

The Rust Omegon binary now owns the `omegon` command. The TS interactive harness needs a distinct npm package name to avoid binary collision. Rename to `omegon-pi` on npm, update all references, deprecate the old `omegon` package.

## Decisions

### Decision: Rename npm package to `omegon-pi`, deprecate `omegon`

**Status:** decided
**Rationale:** Rust binary owns the `omegon` command and product identity. The former TypeScript harness used a distinct name during migration so it could not collide with the native binary.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `package.json` (modified) — name: omegon → omegon-pi, bin: omegon-pi + pi (drop omegon bin name)
- `.github/workflows/publish.yml` (modified) — Update npm view/deprecate/install references from omegon to omegon-pi, add deprecation step for old omegon package
- `extensions/bootstrap/index.ts` (modified) — Update PKG constant and all npm install/view/list references to omegon-pi
- `extensions/version-check.ts` (modified) — Update REPO_NAME if used for npm checks
- `bin/omegon.mjs` (modified) — Rename to bin/omegon-pi.mjs or keep as-is if bin mapping handles it

### Constraints

- Must publish first version under new name before deprecating old name
- Binary command should be omegon-pi not omegon to avoid collision with Rust binary
- pi shim should be preserved for backward compat
