+++
id = "949e3d36-157b-44cb-a7d9-dc1fcb8ff3f6"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# 0.6.6 stability step — subprocess boundary hardening and memory search resilience follow-up

## Overview

Bundle the recursive Omegon subprocess hardening with the immediate follow-up review findings into a release-quality 0.6.6 stability step, then re-assess the resulting diff before release.

## Research

### Current review findings to absorb into 0.6.6

Adversarial review on the current diff identified: (1) memory search now catches all query exceptions and silently returns empty results, masking real FTS/DB failures; (2) the new FTS sanitization may over-normalize code/path-like technical identifiers and weaken recall/precision for symbol-heavy project queries; (3) the new Omegon subprocess resolver is correct for the audited spawn sites but should be treated as a broader stability hardening step and re-assessed after the follow-up fixes; (4) version bump should align with the consolidated 0.6.6 stability step rather than remain incidental noise.

## Decisions

### Decision: 0.6.6 should be a release-quality stability pass, not just a point fix

**Status:** decided
**Rationale:** The subprocess-boundary correction surfaced adjacent reliability issues in project-memory search and release hygiene. Shipping a stability step that absorbs those findings, re-verifies behavior, and then re-assesses the result is lower risk than releasing a narrowly scoped fix with known follow-up concerns still open.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/project-memory/factstore.ts` (modified) — Refine FTS query sanitization and exception handling so malformed user queries are tolerated without masking real storage failures.
- `extensions/project-memory/factstore.test.ts` (modified) — Add regression coverage for apostrophes plus identifier/path-like technical queries and failure transparency.
- `extensions/lib/omegon-subprocess.ts` (modified) — Treat the shared resolver as the canonical internal subprocess entrypoint contract for audited call sites.
- `package.json` (modified) — Move the version bump into the explicit 0.6.6 stability step after the follow-up fixes land.
- `extensions/cleave/dispatcher.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `extensions/cleave/index.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `extensions/project-memory/extraction-v2.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `CHANGELOG.md` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `docs/stability-step-0-6-6.md` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `openspec/changes/stability-step-0-6-6/specs/memory/search-stability.md` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `openspec/changes/stability-step-0-6-6/tasks.md` (modified) — Post-assess reconciliation delta — touched during follow-up fixes

### Constraints

- Malformed FTS input should not crash memory search, but operational/storage failures must remain observable rather than being silently converted to empty results.
- FTS sanitization must preserve useful recall for technical identifier and path-shaped queries common in Omegon's codebase.
- The 0.6.6 stability step should end with a fresh adversarial reassessment of the resulting diff before release.
- The 0.6.6 stability step ends with a fresh targeted reassessment before release.
