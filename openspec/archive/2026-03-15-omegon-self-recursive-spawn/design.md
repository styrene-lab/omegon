+++
id = "ce3e54f7-b823-44ea-a0c9-a851db495ad9"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Recursive subprocesses must invoke Omegon-owned entrypoint, not bare `pi` — Design

## Architecture Decisions

### Decision: Internal subprocess launches must resolve the Omegon entrypoint explicitly

**Status:** decided
**Rationale:** Self-containment requires recursive children and helper subprocesses to re-enter the same Omegon-owned runtime boundary regardless of other binaries on PATH. Bare `pi` is acceptable only as a user-facing compatibility alias, not as an internal execution contract. A shared resolver should compute the canonical executable path or command for the current installation and all internal spawn sites should use it.

## Research Context

### Current subprocess audit

Cleave child execution in `extensions/cleave/dispatcher.ts` still calls `spawn("pi", ...)`. Structured assessment helpers in `extensions/cleave/index.ts` also spawn `pi` directly for bridged spec/design assessment. `extensions/project-memory/extraction-v2.ts` likewise spawns `pi`, including a detached path. Although package.json maps the legacy `pi` alias back into `bin/pi.mjs` and then `bin/omegon.mjs`, these sites still depend on PATH resolving to Omegon's compatibility shim rather than explicitly re-entering the Omegon-owned entrypoint.

## File Changes

- `extensions/cleave/dispatcher.ts` (modified) — Replace bare `pi` child dispatch with shared Omegon executable resolution.
- `extensions/cleave/index.ts` (modified) — Route bridged assessment subprocesses through the shared Omegon executable resolver.
- `extensions/project-memory/extraction-v2.ts` (modified) — Route subprocess extraction fallback through the shared Omegon executable resolver.
- `extensions/lib` (new) — Add a shared helper for resolving the canonical Omegon subprocess executable/argv contract.

## Constraints

- Internal recursive subprocesses must not depend on PATH resolving `pi` to Omegon.
- Compatibility alias `pi` may remain for operators, but internal subprocesses should use the canonical Omegon-owned entrypoint contract.
- Preserve current subprocess flags and non-interactive behavior while changing only executable resolution semantics.
