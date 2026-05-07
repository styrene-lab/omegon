+++
id = "25fa767d-b8c5-4c40-8f9a-1996f78acb6a"
kind = "document"
title = "Idea Layer — Pre-Design Capture Primitive"
status = "resolved"
tags = ["architecture", "lifecycle", "auspex"]
aliases = ["idea-layer"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
dependencies = []
open_questions = []
related = []
+++

# Idea Layer — Pre-Design Capture Primitive

## Overview

Add a freeform "idea" capture layer that sits below the design tree in the lifecycle hierarchy. Ideas are pre-classification (not features, not bugs — just captured thoughts) stored as markdown files in `.omegon/ideas/`. They have a lifecycle: raw → assessed → promoted/parked/rejected. Promotion creates design tree seeds. This subsumes the existing /note command. Paired with a project manifest (`project.json`) that gives Auspex stable identity handles per repo.

## Research

### Schemas and layout draft



## Decisions

### Ideas stored as flat markdown with YAML frontmatter in .omegon/ideas/

**Status:** accepted

**Rationale:** Maximum freeform capture. Filename is the stable ID (kebab-case slug). No index file — directory listing is the index. Agents enumerate with readdir, parse frontmatter for status/tags. Human-editable, git-diffable, grep-friendly.

### Idea lifecycle: raw → assessed → promoted | parked | rejected

**Status:** accepted

**Rationale:** raw = just captured. assessed = agent reviewed, annotated gaps/questions. promoted = spawned design seeds (promotes_to links to node IDs). parked = valid but not actionable. rejected = superseded or doesn't hold up. Promoted ideas stay as provenance, not deleted.

### /idea subsumes /note as the universal capture command

**Status:** accepted

**Rationale:** /note was always a proto-idea without lifecycle. Ideas are the same thing with status tracking and promotion to design seeds. No reason to maintain both primitives.

### Every project is git-backed; remotes are optional

**Status:** accepted

**Rationale:** git.local: true is always true. Remotes map is informational — Auspex reads it for sync options but doesn't enforce any. Empty remotes is first-class, not degraded. Future lightweight git server becomes just another remote entry.

### New projects: Auspex inits a local git repo and scaffolds .omegon/

**Status:** accepted

**Rationale:** Preserves the invariant that every idea has a project home and every project is git-backed. Creating a project is cheap (git init + two files). No orphan ideas concept needed.

### Auspex discovers projects by scanning, then stores explicitly

**Status:** accepted

**Rationale:** Scan is a QoL import mechanism — finds .omegon/ dirs under configured workspace roots. After discovery, projects are stored as explicit path entries in the Auspex registry. No re-scanning on every startup.

### Agent writes a context paragraph on /idea; operator can attach media and references

**Status:** accepted

**Rationale:** Agent captures a paragraph of context from the conversation. Operator can enrich later with images, document references, links. The idea file is markdown so media embeds and links are native.

### Ideas are auto-committed to git on creation

**Status:** accepted

**Rationale:** Immediate durability. If the session crashes or a batch commit fails, the idea is already safe. Commit noise is acceptable — ideas are infrequent compared to code changes and the commit message makes intent clear.

### /note is removed, not aliased — /idea is the sole capture primitive

**Status:** accepted

**Rationale:** Two capture commands with overlapping purpose creates confusion about which to use. Clean break. /idea is the universal primitive with lifecycle.

### Per-repo .omegon/project.json; Auspex registry is a manifest of project references

**Status:** accepted

**Rationale:** Each repo owns its identity via .omegon/project.json. Auspex maintains a registry that references these by path — it does not duplicate or shadow project metadata. Single source of truth per layer.

### Assessment is an explicit command, designed for future cron/schedule/event triggers

**Status:** accepted

**Rationale:** Not automatic on session start — that's a side-effect the operator didn't ask for. Explicit /assess ideas command. Architecture supports future automation via cron, Auspex event hooks, or webhook triggers without changing the assessment logic itself.

### Promotion is unrestricted — one idea can spawn one or many seeds

**Status:** accepted

**Rationale:** Don't restrict cardinality. A short idea may be profound and spawn one seed. A broad idea may decompose into several. The promotes_to array handles both cases. Let the agent and operator decide at promotion time.
