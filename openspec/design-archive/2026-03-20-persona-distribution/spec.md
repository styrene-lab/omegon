+++
id = "433d07cd-45c9-4200-af65-3fcbf5aa5fd5"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Persona distribution — repos, manifests, and URI addressing — Design Spec (extracted)

> Auto-extracted from docs/persona-distribution.md at decide-time.

## Decisions

### Unified plugin system: plugin.toml manifest, one install command for personas/tones/skills/extensions (decided)

Operators shouldn't manage 15 separate install paths. A single plugin.toml manifest with a type field (persona/tone/skill/extension) unifies discovery, installation, and activation. One command: `omegon plugin install <uri>`. Git repos as the distribution primitive — any git URL works, including private repos. Reverse-domain IDs for uniqueness without a central registry. A persona can bundle skills, a default tone, and lightweight tool configs. This is the FOSS Claude Code plugin alternative — but better because plugins can carry knowledge (minds, tone exemplars), not just tools and markdown.
