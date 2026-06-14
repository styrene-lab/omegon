+++
id = "f7acbe5d-5a48-4077-bad2-54a7e08f8d6c"
tags = ["documentation", "index"]
aliases = ["docs-index"]
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Documentation Map

This directory is the durable project knowledge base for Omegon. It contains architecture notes, implementation plans, postmortems, and lifecycle records. It is not the same as the public docs site.

For user-facing docs, edit `site/src/pages/docs/` and the command snippets in `site/snippets/`.

## Start Here

- `README.md` at the repository root: product overview, install, core concepts, and source build path.
- `CONTRIBUTING.md`: branch policy, validation commands, release flow, and workspace layout.
- `EXTENSIONS.md`: extension system overview.
- `EXTENSION_SDK.md`: extension authoring quick start and protocol reference.
- `docs/omegon-install.md`: distribution notes, Linux glibc caveats, and update contract.
- `docs/provider-credential-map.md`: provider auth and credential behavior.
- `docs/omegon-session.md`: session persistence behavior.
- `docs/cleave.md`: parallel worktree orchestration.
- `docs/sentry.md`: long-running task executor, triggers, budgets, and auto routing.
- `docs/n8n-sentry-submission.md`: planned external workflow submission API for n8n, Flynt, Auspex, and future protocol adapters.
- `docs/omegon-browser-extension.md`: native browser automation extension backed by Vercel agent-browser.
- `docs/armory-discovery.md`: unified discovery model for browsing upstream extensions, plugins, skills, and catalog agents.
- `docs/project-memory.md`: project memory behavior.
- `docs/openapi-tools.md`: project-local OpenAPI specs compiled into agent tools.
- `docs/prompt-and-user-command-surfaces.md`: reusable prompt definitions, `/prompt` routing, safety verdicts, and user-defined command aliases.

## Directory Boundaries

- `docs/`: durable architecture and implementation docs that should remain readable over time.
- `design/`: older design notes and exploratory material.
- `openspec/`: active and archived OpenSpec lifecycle artifacts.
- `site/`: public documentation site source and generated `dist/`.
- `ai/benchmarks/`: benchmark tasks and recorded runs.

When adding a new long-lived document, prefer `docs/` and include frontmatter. When adding public-facing guidance, update the Astro page in `site/src/pages/docs/` and use snippets for commands that appear in more than one place.

- `docs/acp-surface.md`: canonical ACP integration contract for Zed, Flynt, and external clients.
