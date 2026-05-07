+++
id = "982c4e4a-c81c-491b-a491-f83a756a9436"
kind = "document"
title = "Recursive subprocesses must invoke Omegon-owned entrypoint, not bare `pi`"
status = "implemented"
tags = ["runtime", "cleave", "subprocess", "binary", "bug", "self-containment"]
aliases = ["omegon-self-recursive-spawn"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
branches = []
issue_type = "bug"
open_questions = []
openspec_change = "omegon-self-recursive-spawn"
parent = "omegon-binary-identity"
priority = "1"
+++

# Recursive subprocesses must invoke Omegon-owned entrypoint, not bare `pi`

## Overview

Ensure all internal recursive subprocess launches re-enter the Omegon-owned executable boundary explicitly, rather than depending on PATH resolution of the legacy `pi` compatibility alias. Audit cleave and adjacent subprocess sites, then route them through a shared Omegon executable resolver so side-by-side installs cannot escape the self-contained runtime boundary.

## Research

### Current subprocess audit

Cleave child execution in `extensions/cleave/dispatcher.ts` still calls `spawn("pi", ...)`. Structured assessment helpers in `extensions/cleave/index.ts` also spawn `pi` directly for bridged spec/design assessment. `extensions/project-memory/extraction-v2.ts` likewise spawns `pi`, including a detached path. Although package.json maps the legacy `pi` alias back into `bin/pi.mjs` and then `bin/omegon.mjs`, these sites still depend on PATH resolving to Omegon's compatibility shim rather than explicitly re-entering the Omegon-owned entrypoint.

## Decisions

### Decision: Internal subprocess launches must resolve the Omegon entrypoint explicitly

**Status:** decided
**Rationale:** Self-containment requires recursive children and helper subprocesses to re-enter the same Omegon-owned runtime boundary regardless of other binaries on PATH. Bare `pi` is acceptable only as a user-facing compatibility alias, not as an internal execution contract. A shared resolver should compute the canonical executable path or command for the current installation and all internal spawn sites should use it.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/cleave/dispatcher.ts` (modified) — Replace bare `pi` child dispatch with shared Omegon executable resolution.
- `extensions/cleave/index.ts` (modified) — Route bridged assessment subprocesses through the shared Omegon executable resolver.
- `extensions/project-memory/extraction-v2.ts` (modified) — Route subprocess extraction fallback through the shared Omegon executable resolver.
- `extensions/lib` (new) — Add a shared helper for resolving the canonical Omegon subprocess executable/argv contract.
- `extensions/lib/omegon-subprocess.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `docs/omegon-self-recursive-spawn.md` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `openspec/changes/omegon-self-recursive-spawn/specs/runtime/subprocess-entrypoint.md` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `openspec/changes/omegon-self-recursive-spawn/tasks.md` (modified) — Post-assess reconciliation delta — touched during follow-up fixes

### Constraints

- Internal recursive subprocesses must not depend on PATH resolving `pi` to Omegon.
- Compatibility alias `pi` may remain for operators, but internal subprocesses should use the canonical Omegon-owned entrypoint contract.
- Preserve current subprocess flags and non-interactive behavior while changing only executable resolution semantics.
- Internal recursive subprocesses must resolve the Omegon-owned entrypoint explicitly rather than relying on PATH to select the `pi` compatibility alias.
- Subprocess flag behavior and non-interactive semantics must remain unchanged while executable resolution changes.
