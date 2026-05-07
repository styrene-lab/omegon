+++
id = "075ec0ea-6b90-4fb1-a5f1-7d822e2ca789"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# omegon.styrene.dev/docs — documentation and architecture sub-site

## Overview

Assess adding a /docs sub-path to omegon.styrene.dev for documentation, architecture overview, and marketing content. Currently the site is a single-page landing with install.sh. The question is what to put there, how to build it, and how to keep it current with the 160 design docs, 25 baseline specs, 12 skills, and 67 archived changes that already exist in the repo.

## Research

### Content inventory and audience segmentation

**Tier 1: User-facing docs (must publish)**
- Getting started guide (doesn't exist yet — README is 54 lines, needs expansion)
- CHANGELOG (171 lines, auto-publish on release)
- Skills reference (12 skills, 3183 lines total — these are operator-facing guides for cleave, git, openspec, security, etc.)
- Command reference (slash commands, tool descriptions)
- CONTRIBUTING.md (210 lines)

**Tier 2: Architecture showcase (marketing/developer interest)**
- Design tree visualization — 161 nodes, 87 implemented. This is genuinely unique and impressive.
- Architecture overview — the three-axis routing model, dual lifecycle, memory system, cleave orchestrator
- Selected design decisions — the interesting ones like jj co-location, monorepo migration, fail-closed defaults
- OpenSpec baseline specs (25) — show what spec-driven development looks like

**Tier 3: Internal (stays in repo)**
- Memory facts (1865 entries — agent-internal knowledge)
- Active OpenSpec changes (ephemeral work in progress)
- OpenSpec archive (67 completed changes — historical, not user-relevant)
- Session episodes (agent work logs)

**Audience segments:**
1. **Operators** — people installing and using omegon. Need: getting started, command reference, skills docs, changelog.
2. **Developers** — people building on or contributing to omegon. Need: architecture overview, extension API, contributing guide.
3. **Curious engineers** — people evaluating agent harnesses. Need: architecture showcase, design tree viz, what makes this different.

### Build system options

**Option A: Astro static build (like styrene.io)**
Proven pattern — styrene.io already uses Astro 5 with remark-wikilinks, pagefind search, force-directed local graph. Could reuse or fork the styrene site structure. Build on CI, produce static HTML, bake into the same nginx container (or a separate one). Supports markdown content collections, MDX, component islands.

Pros: Rich features (search, graph, sidebar), proven in the organization, markdown-native.
Cons: Another build toolchain. ~200MB node_modules. Separate from the Rust binary's own web dashboard.

**Option B: Extend existing nginx container with hand-written HTML**
Add more static HTML pages (docs.html, architecture.html, etc.) to core/site/. Hand-craft with the existing Alpharius CSS. Simple, no build step.

Pros: Zero toolchain. Ships in the same container. Instant updates.
Cons: Doesn't scale past ~5 pages. No search. No sidebar navigation. No markdown rendering.

**Option C: mdserve / embedded web dashboard serves docs**
The Rust binary already has an embedded web dashboard (axum). Could add a /docs route that renders the design tree and markdown docs. Users run `omegon serve` to get a local docs server.

Pros: Single binary serves everything. Design tree is live, not static. Deeply integrated.
Cons: Requires running omegon. Not publicly accessible. Not useful for the marketing use case.

**Option D: Hybrid — Astro for public site, embedded dashboard for live view**
Public site at omegon.styrene.dev/docs is Astro-built from repo content. The embedded web dashboard (omegon serve) gives the live interactive experience. Same content, two renderers.

Pros: Best of both worlds. Public docs for discovery, live dashboard for operators.
Cons: Two rendering paths for the same content.

**Recommendation: Option D (Astro for public, mdserve for local) with Option A as the immediate step.** The public site needs to exist for marketing and operator onboarding. The live dashboard is a separate concern. Start with Astro, reuse the styrene.io infrastructure.

## Decisions

### Decision: Public docs site publishes Tier 1 + selected Tier 2 content; Tier 3 stays repo-internal

**Status:** decided
**Rationale:** User-facing docs (getting started, commands, skills, changelog) are essential for adoption. Architecture showcase (design tree viz, selected decisions, three-axis model) differentiates omegon from other agent harnesses and serves the marketing purpose. Internal state (memory facts, active changes, episodes) has no public audience and would be noise.

### Decision: Astro static build for public site, built on CI and served from the same omegon-site container

**Status:** decided
**Rationale:** Astro is proven in the org (styrene.io), handles markdown content collections natively, supports pagefind search, and produces static HTML that fits in the existing nginx container. The build adds to the CI workflow but the runtime cost is zero (same nginx pod). A new site/ directory in the omegon repo holds the Astro project alongside the existing single-page landing.

### Decision: Design tree rendered as a static force-directed SVG at build time, with link to live dashboard for interactive exploration

**Status:** decided
**Rationale:** A pre-rendered graph captures the impressive scale (184 nodes) without requiring JavaScript or a running server. The build script reads docs/*.md frontmatter, generates a D3 force layout, and emits an SVG. The page includes a note that operators can run `omegon serve` for the live interactive version. This keeps the public site fully static while acknowledging the live experience exists.

### Decision: CHANGELOG auto-published at /changelog on every release, generated from CHANGELOG.md at build time

**Status:** decided
**Rationale:** The CHANGELOG already follows conventional format. The Astro build reads core/CHANGELOG.md as a content page, renders it at /changelog. The CI workflow runs on both site/ changes and on release tags (which bump CHANGELOG.md). This means every release automatically updates the public changelog with zero manual steps.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `site/` (new) — New Astro project root (separate from core/site/ which is the landing page)
- `site/astro.config.mjs` (new) — Astro config with markdown, content collections, base path
- `site/src/content/config.ts` (new) — Content collection definitions for docs, skills, architecture
- `site/src/layouts/Docs.astro` (new) — Docs layout with sidebar navigation, Alpharius theme
- `site/src/pages/docs/[...slug].astro` (new) — Dynamic route for all doc pages
- `site/src/pages/changelog.astro` (new) — Changelog page rendering CHANGELOG.md
- `site/scripts/build-design-tree.mjs` (new) — Node script: reads docs/*.md frontmatter, generates force-directed SVG
- `core/site/Containerfile` (modified) — Updated to COPY Astro dist output alongside landing page
- `.github/workflows/site.yml` (modified) — Updated to run Astro build before container build

### Constraints

- Astro output must coexist with existing index.html and install.sh in the nginx container
- Design tree SVG generated at build time from docs/*.md frontmatter
- Skills rendered from skills/*/SKILL.md into /docs/skills/
- CHANGELOG rendered from core/CHANGELOG.md into /changelog
- Alpharius color palette must match the existing landing page CSS variables
- No client-side JavaScript required for core docs reading (static HTML)
- Container image stays under 50MB
