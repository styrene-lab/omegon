+++
id = "97181927-6a03-44b2-8a27-f71b49ff27e7"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# compaction-policy-hotfix — Design

## Spec-Derived Architecture

### project-memory/compaction

- **Normal compaction must not silently prefer heavy local inference** (added) — 3 scenarios
- **Compaction summaries must sanitize ephemeral clipboard temp paths** (added) — 2 scenarios

## Scope

In scope:
- Disable local-first compaction as the default project-memory behavior.
- Make normal day-to-day effort tiers use cloud-first compaction policy.
- Preserve local compaction as an explicit local-tier or retry fallback path.
- Redact transient `pi-clipboard-*.png` temp paths from local compaction prompt text.
- Add regression tests for policy selection and path sanitization.

Out of scope:
- Full operator-profile-driven compaction routing.
- Benchmarking or hardware-aware local model selection.
- Changing extraction model policy.

## File Changes

- `extensions/project-memory/compaction-policy.ts` (new) — pure helpers for local-compaction interception policy and clipboard temp-path redaction.
- `extensions/project-memory/index.ts` (modified) — use policy helper for interception checks, sanitize local compaction prompt text, and apply effort compaction policy directly.
- `extensions/project-memory/types.ts` (modified) — default `compactionLocalFirst` to `false`.
- `extensions/project-memory/compaction-policy.test.ts` (new) — regression coverage for policy decisions and clipboard temp-path redaction.
- `extensions/effort/tiers.ts` (modified) — switch normal work tiers to cloud-first compaction.
- `extensions/effort/tiers.test.ts` (modified) — update expectations for revised compaction tiers.
