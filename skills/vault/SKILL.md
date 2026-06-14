+++
id = "8d7961f6-4742-416f-89eb-bef9f6cc12f6"
name = "vault"
description = "Interlinked markdown conventions for wikis and knowledge bases"
tags = []
aliases = []
activation = "domain_detected"
profile = ["docs"]
project_signals = ["*.md", "docs/**/*.md"]
+++

# Vault Skill — Interlinked Markdown Conventions

Write markdown that renders beautifully in mdserve, Obsidian, and GitHub.

## Wikilink Syntax

Use `[[wikilinks]]` to create navigable connections between documents:

```markdown
See [[vision]] for the big picture.
Related: [[design-tree|Design Exploration Tree]]
```

- `[[target]]` — links to the file whose slug matches `target`
- `[[target|Display Text]]` — links with custom display text
- Slugs are case-insensitive, spaces become hyphens
- Both filename-only (`[[vision]]`) and path slugs (`[[docs/vision]]`) resolve

Unresolved wikilinks render as styled concept references (italic, muted) — they're safe to use as forward references or concept tags.

## Frontmatter

Always include YAML frontmatter with at least `title`:

```yaml
---
title: Architecture Decision Record
status: decided
tags: [architecture, crdt, storage]
---
```

Common fields:
- `title` — displayed in sidebar and graph labels
- `status` — seed, exploring, decided, blocked, deferred
- `tags` — for filtering and grouping
- `date` — ISO date for temporal ordering

## File Organization

Structure directories so the hierarchy IS the navigation:

```
project/
  ai/
    design/           ← design tree nodes
    memory/           ← memory index (generated)
  openspec/
    changes/
      feature-name/
        proposal.md   ← what and why
        design.md     ← how (architecture decisions)
        tasks.md      ← work breakdown
        specs/        ← Given/When/Then scenarios
  docs/               ← long-lived documentation
```

## Graph-Friendly Patterns

- **Link generously** — every doc should link to at least one other doc
- **Backlink naturally** — if A links to B, B should mention A
- **Hub pages** — create index/MOC (Map of Content) files that link to all docs in a directory
- **Consistent naming** — use kebab-case for filenames (`architecture-decisions.md`)

## Running the Viewer

```bash
# Serve current project (recursive, all .md files)
mdserve .

# Serve specific directory
mdserve openspec/

# Auto-open browser
mdserve . --open

# Custom port
mdserve . --port 8080
```

The viewer provides:
- Sidebar navigation with directory tree
- Live reload on file changes (WebSocket)
- Interactive graph view at `/graph`
- Theme picker (including Styrene dark theme)
- Per-project persistent settings

## Installation

```bash
# From source (requires Rust toolchain)
cargo install --git https://github.com/cwilson613/mdserve --branch feature/wikilinks-graph

# Or build locally
git clone https://github.com/cwilson613/mdserve
cd mdserve && cargo install --path .
```
