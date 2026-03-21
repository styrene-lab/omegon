---
id: persona-system
title: Persona System — domain-expert identities with dedicated mind stores
status: implemented
tags: [architecture, persona, memory, skills, domain-expertise, strategic]
open_questions: []
issue_type: epic
priority: 2
---

# Persona System — domain-expert identities with dedicated mind stores

## Overview

Agent personas as named identities that bundle: a markdown directive (personality, expertise, behavioral guidelines), a dedicated mind/memory store (pre-populated domain knowledge), and a set of activated skills/extensions/tools. Personas are addressable by name/id/URI, toggleable in settings, and composable with the existing memory system. The key insight: a PCB design persona isn't just markdown pretending to be engineering — it's a mind pre-populated with actual research, standards, and domain constraints before the session starts.

## Research

### Anatomy of a persona: the Tutor example (SciencePetr)

A well-structured persona prompt has these layers:

1. **Identity declaration** — "You are a patient, skilled tutor"
2. **Core principles** (behavioral axioms) — Active Learning, Socratic Questioning, Cognitive Load Management, Growth Mindset, Scaffolding, Targeted Feedback, Self-Pacing
3. **Interaction style** — concise, plain language, short responses, mirror formality
4. **Session structure** — Assess → Orient → Guide → Check → Summarize
5. **Anti-patterns** — explicit "What NOT To Do" section
6. **Accuracy policy** — honesty about uncertainty, no hallucination

Key insight: the tutor persona doesn't just change *what* the agent says — it changes *how it thinks*. It inverts the default behavior (explain everything) into a Socratic mode (ask questions, scaffold, never lecture). This is a cognitive mode shift, not just a tone change.

A persona for PCB design would follow the same structure but with engineering-specific principles: reference standards (IPC-2221), design rule checks, thermal analysis methodology, and a mind pre-populated with component datasheets and design constraints — not just markdown telling the agent to "be an electrical engineer".

### Landscape: how other harnesses handle persona/skills

**Claude Code / pi**: `AGENTS.md` files for project directives, `SKILL.md` files for domain knowledge. Skills are markdown loaded on-demand. No persona concept — just system prompt injection.

**Microsoft Agent Framework**: `SKILL.md` discovery via recursive directory scan, description-based routing. Agent decides when to load a skill based on its description. Closest to our skills system.

**OpenAI Codex**: Repository knowledge as system of record. Execution plans checked into the repo. No explicit persona, but "harness engineering" shapes behavior through context.

**Claude Projects**: System prompt in project instructions. The tutor example uses this — persona as a project-level system prompt. Simple but no mind store, no tool binding, no composability.

**Etienne**: Custom MCP tools per project, dynamic Python tool discovery. Persona-adjacent but tool-focused, not identity-focused.

**Common gap everywhere**: No harness treats persona as a first-class entity with its own durable memory. They're all markdown-in, markdown-out. The mind store is what differentiates Omegon's approach — a persona that remembers what it's learned and carries domain knowledge across sessions.

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
seed_episodes = "mind/episodes.jsonl"

[persona.skills]
# Skills to auto-activate when this persona is loaded
activate = ["pedagogy", "assessment"]
# Skills to deactivate (if normally loaded)
deactivate = []

[persona.tools]
# Tool profiles to apply
profile = "tutor"
# Tools to force-enable
enable = []
# Tools to force-disable (tutor shouldn't have code execution)
disable = ["bash", "write"]

[persona.style]
# Optional: UI customization
badge = "📚"
accent_color = "#2ab4c8"
```

The layering model:
1. **Core directives** (always-on) — anti-sycophancy, evidence-based epistemology, etc.
2. **Persona directive** (PERSONA.md) — behavioral principles, interaction style, session structure
3. **Persona mind** (facts.jsonl) — pre-populated domain knowledge
4. **Persona skills** — markdown guidances activated for this domain
5. **Persona tools** — tool profile overrides
6. **Project memory** (existing) — project-specific facts accumulate on top

Activation via: settings toggle, `/persona <name>` command, or omega inference.

### Relationship to existing Omegon systems

The persona system connects to almost every existing subsystem:

- **Memory system** — Mind stores are the same format as project memory (facts.jsonl, episodes.jsonl). Persona activation merges persona facts into the working memory layer. Deactivation removes them. The existing `memory_store`, `memory_recall`, `memory_query` tools work against the merged view.

- **Skills system** — Existing `SKILL.md` files are already persona-adjacent. A persona bundles skill activation. The skills-repo-format work enables community skills that personas can reference.

- **Tool profiles** — `manage_tools` already supports profiles. A persona maps to a tool profile — the tutor persona disables bash/write, the PCB persona enables EDA tools.

- **Context class / routing** — A persona could influence default routing. The tutor persona might pin to a high thinking level (Socratic questioning needs reasoning). A code-focused persona might default to victory tier for speed.

- **Settings / Profile** — Persona selection persists in the Rust Profile struct. `active_persona: Option<String>` plus `persona_overrides: HashMap<String, Value>`.

- **Dashboard** — Active persona shown in footer badge. Settings panel shows persona picker with description and mind store size.

- **Design tree** — Persona definitions are design artifacts. The design tree can track which persona was active during exploration (like jj change IDs binding to facts).

## Decisions

### Decision: Persona activation automatically loads bundled skills, tone, and tool profile

**Status:** decided
**Rationale:** The plugin.toml manifest declares what a persona bundles. On activation, the harness loads the persona directive, merges the mind store, activates declared skills, applies the tool profile, and optionally sets a default tone. This is atomic — one action equips the full loadout. Deactivation reverses it cleanly.

## Open Questions

*No open questions.*
