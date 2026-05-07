+++
id = "cc291f8d-90b7-47e5-b597-7b07b624d43b"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Persona System — domain-expert identities with dedicated mind stores — Design Spec (extracted)

> Auto-extracted from docs/persona-system.md at decide-time.

## Decisions

### Persona activation automatically loads bundled skills, tone, and tool profile (decided)

The plugin.toml manifest declares what a persona bundles. On activation, the harness loads the persona directive, merges the mind store, activates declared skills, applies the tool profile, and optionally sets a default tone. This is atomic — one action equips the full loadout. Deactivation reverses it cleanly.

## Research Summary

### Anatomy of a persona: the Tutor example (SciencePetr)

A well-structured persona prompt has these layers:

1. **Identity declaration** — "You are a patient, skilled tutor"
2. **Core principles** (behavioral axioms) — Active Learning, Socratic Questioning, Cognitive Load Management, Growth Mindset, Scaffolding, Targeted Feedback, Self-Pacing
3. **Interaction style** — concise, plain language, short responses, mirror formality
4. **Session structure** — Assess → Orient → Guide → Check → Summarize
5. **Anti-patterns** — explicit "What NOT To Do" sectio…

### Landscape: how other harnesses handle persona/skills

**Claude Code / pi**: `AGENTS.md` files for project directives, `SKILL.md` files for domain knowledge. Skills are markdown loaded on-demand. No persona concept — just system prompt injection.

**Microsoft Agent Framework**: `SKILL.md` discovery via recursive directory scan, description-based routing. Agent decides when to load a skill based on its description. Closest to our skills system.

**OpenAI Codex**: Repository knowledge as system of record. Execution plans checked into the repo. No expl…

### Proposed persona data model

A persona is defined as:

```toml
# persona.toml
[persona]
name = "tutor"
id = "com.sciencepetr.tutor"   # reverse-domain URI
version = "1.0.0"
description = "Socratic tutor — guides through questioning, never lectures"

[persona.identity]
# The behavioral directive (injected into system prompt)
directive = "PERSONA.md"

[persona.mind]
# Pre-populated facts to seed the mind store on activation
seed_facts = "mind/facts.jsonl"
# Optional: curated episodes for domain context
seed_episodes = "mind/e…

### Relationship to existing Omegon systems

The persona system connects to almost every existing subsystem:

- **Memory system** — Mind stores are the same format as project memory (facts.jsonl, episodes.jsonl). Persona activation merges persona facts into the working memory layer. Deactivation removes them. The existing `memory_store`, `memory_recall`, `memory_query` tools work against the merged view.

- **Skills system** — Existing `SKILL.md` files are already persona-adjacent. A persona bundles skill activation. The skills-repo-format…
