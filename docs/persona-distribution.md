---
id: persona-distribution
title: Persona distribution — repos, manifests, and URI addressing
status: implemented
parent: persona-system
open_questions: []
issue_type: feature
---

# Persona distribution — repos, manifests, and URI addressing

## Overview

> Parent: [Persona System — domain-expert identities with dedicated mind stores](persona-system.md)
> Spawned from: "How are personas distributed? Git repos with a manifest (persona.toml)? URI-addressable for third-party publishing? Same install path as skills repos?"

*To be explored.*

## Decisions

### Decision: Unified plugin system: plugin.toml manifest, one install command for personas/tones/skills/extensions

**Status:** decided
**Rationale:** Operators shouldn't manage 15 separate install paths. A single plugin.toml manifest with a type field (persona/tone/skill/extension) unifies discovery, installation, and activation. One command: `omegon plugin install <uri>`. Git repos as the distribution primitive — any git URL works, including private repos. Reverse-domain IDs for uniqueness without a central registry. A persona can bundle skills, a default tone, and lightweight tool configs. This is the FOSS Claude Code plugin alternative — but better because plugins can carry knowledge (minds, tone exemplars), not just tools and markdown.

## Open Questions

*No open questions.*
