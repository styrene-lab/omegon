+++
id = "e1624b1e-8e5d-4f67-b971-84c861bad263"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Native SVG diagram backend MVP

## Overview

Implement a tightly scoped native diagram backend inside extensions/render that generates document-bound SVG and PNG from constrained motif-based specs, alongside existing D2 and Excalidraw tooling.

## Decisions

### Decision: Implement the native backend as a sibling path inside extensions/render

**Status:** decided
**Rationale:** The new backend should coexist with D2 and Excalidraw rather than replacing them, so operators have multiple fit-for-purpose rendering paths under one extension.

### Decision: Scope the MVP to motif-based document diagrams rendered as SVG and optionally PNG

**Status:** decided
**Rationale:** A constrained motif compiler is enough to prove the native architecture without introducing a general relation grammar, interactive editor semantics, or broad layout complexity.

### Decision: Use a native SVG backend with Node-native rasterization

**Status:** decided
**Rationale:** Producing SVG directly gives Omegon deterministic geometry and testable output, while Node-native rasterization avoids browser/Playwright dependencies for the new backend.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/render/native-diagrams/` (new) — New modules for constrained spec parsing, motif compilation, scenegraph generation, SVG serialization, and PNG export.
- `extensions/render/index.ts` (modified) — Register a new native diagram tool alongside existing render tools and share output-path conventions.
- `skills/style/SKILL.md` (modified) — Document when the native backend should be chosen over D2 or Excalidraw.
- `extensions/render/native-diagrams/*.test.ts` (new) — Add tests for parsing, motif rendering, SVG output, and PNG export plumbing.
- `extensions/render/native-diagrams/spec.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `extensions/render/native-diagrams/motifs.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `extensions/render/native-diagrams/raster.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `extensions/render/native-diagrams/index.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `extensions/render/native-diagrams/index.test.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `package.json` (modified) — Post-assess reconciliation delta — touched during follow-up fixes

### Constraints

- Keep the MVP narrowly scoped: motif-based document diagrams only.
- Do not replace D2 or Excalidraw; add a sibling native backend inside extensions/render.
- Prefer Node-native SVG and PNG generation; avoid browser/editor dependencies for the new path.
- Keep the initial motif set small and deterministic; defer general relation grammar and broad ELK integration.
