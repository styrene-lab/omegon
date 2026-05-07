+++
id = "18c00b62-f3cc-40ca-b315-b40589429012"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Automatic documentation generation — design tree and implementation drive site content

## Overview

The docs site (omegon.styrene.dev) has hand-authored Astro pages that go stale as the codebase evolves. Design tree nodes, OpenSpec changes, the CHANGELOG, tool definitions, and slash commands all exist as structured data in the repo — but none of it flows into the site automatically.

This feature would generate documentation pages from the existing structured artifacts:

- **Design tree → architecture docs**: 213 design nodes with research, decisions, and status could render as a browsable architecture reference. The SVG visualization already exists but the narrative content doesn't flow through.
- **Tool registry → tool reference**: 48 tool definitions with descriptions and parameter schemas could generate a complete tool reference page.
- **Slash commands → command reference**: the COMMANDS table in the TUI could generate a commands page.
- **CHANGELOG → release notes**: git-cliff already generates the changelog; the site just needs to render it without manual intervention.
- **OpenSpec changes → implementation status**: active changes with task progress could show on a project status page.

The CI pipeline already triggers on docs/ and site/ changes. The gap is a build step that reads structured repo data and emits Astro-compatible content before the site builds.

Motivating incident: 15 RC releases shipped in a single session with major features (version switcher, tool registry, OAuth fixes, TUI hardening) but none of it appeared on the docs site because the Astro pages are manually authored.
