+++
id = "ecdaa028-cba5-4609-8ed7-a11298c1b5e5"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Persona mind stores — dedicated memory layers per persona identity — Design Spec (extracted)

> Auto-extracted from docs/persona-mind-store.md at decide-time.

## Decisions

### Layered merge: persona mind as a distinct memory layer between working memory and project memory (decided)

Option C — persona mind is seeded from facts.jsonl at install, loaded as a layer on activation, grows during sessions with domain-relevant facts, and is portable across projects (global, not project-scoped). Injection priority: working memory (pinned) → persona mind layer → project memory → Lex Imperialis (structural). Resettable to seed state. Same facts.jsonl format with added source/tags fields for auditability.
