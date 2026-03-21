---
id: persona-mind-store
title: Persona mind stores — dedicated memory layers per persona identity
status: implemented
parent: persona-system
open_questions: []
issue_type: feature
---

# Persona mind stores — dedicated memory layers per persona identity

## Overview

> Parent: [Persona System — domain-expert identities with dedicated mind stores](persona-system.md)
> Spawned from: "What is the relationship between persona mind stores and existing project memory? Separate DBs? Namespaced sections? A persona memory layer that merges on activation?"

*To be explored.*

## Decisions

### Decision: Layered merge: persona mind as a distinct memory layer between working memory and project memory

**Status:** decided
**Rationale:** Option C — persona mind is seeded from facts.jsonl at install, loaded as a layer on activation, grows during sessions with domain-relevant facts, and is portable across projects (global, not project-scoped). Injection priority: working memory (pinned) → persona mind layer → project memory → Lex Imperialis (structural). Resettable to seed state. Same facts.jsonl format with added source/tags fields for auditability.

## Open Questions

*No open questions.*
