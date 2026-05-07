+++
id = "e33a094b-d0a1-418d-995d-76d863505e5e"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Design: Scenario-First Task Generation

## Architecture Decisions

- Task groups are organized by spec domain, not file layer
- Spec-domain annotations (`<!-- specs: domain -->`) in tasks.md headers make matching deterministic
- Orphan scenarios (matching zero children) are auto-injected with `⚠️ CROSS-CUTTING` marker
- Overlapping file scope between children is allowed — cleave merge conflict detection handles collisions
- Scenario matching priority: annotation match → scope-based match → word-overlap fallback
- Skill instructions drive generation; code handles matching and orphan safety net

## File Changes

- `extensions/cleave/openspec.ts` (modified) — add `specDomains: string[]` to TaskGroup, parse `<!-- specs: ... -->` in parseTasksFile
- `extensions/cleave/workspace.ts` (modified) — rewrite buildDesignSection: annotation-first matching, orphan detection, auto-inject with marker
- `extensions/cleave/openspec.test.ts` (modified) — tests for annotation parsing, orphan detection, auto-injection target selection
- `extensions/cleave/workspace.test.ts` (new) — tests for buildDesignSection rewrite
- `skills/openspec/SKILL.md` (modified) — add scenario-first grouping guidance and examples
- `skills/cleave/SKILL.md` (modified) — document spec-domain annotations and orphan behavior

## Scenario Matching Flow

```
1. Parse tasks.md → TaskGroup[] (each with specDomains from annotation)
2. Read spec scenarios from specs/*.md
3. For each scenario:
   a. Check if any child's specDomains includes the scenario's domain → assign
   b. Else check if any child's file scope includes likely enforcement files → assign
   c. Else word-overlap fallback → assign to best match
   d. If still no match → auto-inject into closest child by scope, mark ⚠️ CROSS-CUTTING
4. After all matching: every scenario is in at least one child (invariant)
```

## Orphan Injection Target Selection

1. Parse the scenario's `When` clause for file/function references
2. Find the child whose `scope` array contains a matching file path
3. If no scope match, fall back to highest word overlap between scenario text and child description
4. If still no match (shouldn't happen), inject into the last child (integration tasks tend to land there)
