+++
id = "9f5a615f-36de-4da7-b5a2-0634b0c35623"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Compaction policy hotfix — avoid heavy local default and redact clipboard temp paths

## Overview

Hotfix local compaction policy so routine sessions do not silently route to heavy local Ollama models for compaction, while preserving local fallback/retry paths. Also redact transient pi-clipboard temp image paths from compaction input/summary text to reduce noise and stale error/path leakage.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/project-memory/compaction-policy.ts` (new) — Pure helpers for local compaction interception policy and clipboard temp-path redaction
- `extensions/project-memory/index.ts` (modified) — Use compaction policy helper, sanitize local compaction prompt text, and apply cloud-first defaults
- `extensions/project-memory/types.ts` (modified) — Disable local-first compaction by default
- `extensions/project-memory/compaction-policy.test.ts` (new) — Regression tests for interception policy and clipboard path redaction
- `extensions/effort/tiers.ts` (modified) — Make normal day-to-day effort tiers cloud-first for compaction
- `extensions/effort/tiers.test.ts` (modified) — Update tier expectations for compaction policy change

### Constraints

- Keep local compaction available for explicit local tiers and retry fallback after cloud failure.
- Only redact transient pi-clipboard temp image paths; preserve ordinary repository file paths in compaction context.
