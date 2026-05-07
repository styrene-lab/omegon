+++
id = "7e27e73b-4754-4023-ad42-113e43807970"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# omegon.styrene.dev/docs — documentation and architecture sub-site — Design Spec (extracted)

> Auto-extracted from docs/omegon-site-docs.md at decide-time.

## Decisions

### Public docs site publishes Tier 1 + selected Tier 2 content; Tier 3 stays repo-internal (decided)

User-facing docs (getting started, commands, skills, changelog) are essential for adoption. Architecture showcase (design tree viz, selected decisions, three-axis model) differentiates omegon from other agent harnesses and serves the marketing purpose. Internal state (memory facts, active changes, episodes) has no public audience and would be noise.

### Astro static build for public site, built on CI and served from the same omegon-site container (decided)

Astro is proven in the org (styrene.io), handles markdown content collections natively, supports pagefind search, and produces static HTML that fits in the existing nginx container. The build adds to the CI workflow but the runtime cost is zero (same nginx pod). A new site/ directory in the omegon repo holds the Astro project alongside the existing single-page landing.

### Design tree rendered as a static force-directed SVG at build time, with link to live dashboard for interactive exploration (decided)

A pre-rendered graph captures the impressive scale (184 nodes) without requiring JavaScript or a running server. The build script reads docs/*.md frontmatter, generates a D3 force layout, and emits an SVG. The page includes a note that operators can run `omegon serve` for the live interactive version. This keeps the public site fully static while acknowledging the live experience exists.

### CHANGELOG auto-published at /changelog on every release, generated from CHANGELOG.md at build time (decided)

The CHANGELOG already follows conventional format. The Astro build reads core/CHANGELOG.md as a content page, renders it at /changelog. The CI workflow runs on both site/ changes and on release tags (which bump CHANGELOG.md). This means every release automatically updates the public changelog with zero manual steps.

## Research Summary

### Content inventory and audience segmentation

**Tier 1: User-facing docs (must publish)**
- Getting started guide (doesn't exist yet — README is 54 lines, needs expansion)
- CHANGELOG (171 lines, auto-publish on release)
- Skills reference (12 skills, 3183 lines total — these are operator-facing guides for cleave, git, openspec, security, etc.)
- Command reference (slash commands, tool descriptions)
- CONTRIBUTING.md (210 lines)

**Tier 2: Architecture showcase (marketing/developer interest)**
- Design tree visualization — 161 nodes, 87 imp…

### Build system options

**Option A: Astro static build (like styrene.io)**
Proven pattern — styrene.io already uses Astro 5 with remark-wikilinks, pagefind search, force-directed local graph. Could reuse or fork the styrene site structure. Build on CI, produce static HTML, bake into the same nginx container (or a separate one). Supports markdown content collections, MDX, component islands.

Pros: Rich features (search, graph, sidebar), proven in the organization, markdown-native.
Cons: Another build toolchain. ~200MB n…
