+++
id = "ba03e6c8-ed51-4789-84f9-5115b9d08fdc"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Tasks: Scenario-First Task Generation

## 1. Annotation Parsing and TaskGroup Extension
<!-- specs: cleave/spec -->
- [x] Add `specDomains: string[]` field to TaskGroup interface in openspec.ts
- [x] Parse `<!-- specs: domain/name, ... -->` comments in parseTasksFile — extract from line following group header
- [x] Handle edge cases: no annotation, multiple domains, whitespace variations
- [x] Add tests for annotation parsing (3 scenarios from spec)

## 2. Scenario Matching Rewrite
<!-- specs: cleave/spec -->
- [x] Rewrite scenario-to-child matching in workspace.ts buildDesignSection
- [x] Implement 3-tier priority: annotation match → scope match → word-overlap fallback
- [x] Extract matching into a standalone function for testability
- [x] Add tests for annotation-first matching, scope fallback, and word-overlap fallback

## 3. Orphan Detection and Auto-Inject
<!-- specs: cleave/spec -->
- [x] After per-child matching, collect scenarios that matched zero children
- [x] Implement injection target selection: parse When clause for file/function refs, match against child scopes, fall back to word overlap
- [x] Inject orphaned scenarios with `⚠️ CROSS-CUTTING` prefix
- [x] Add tests for orphan detection, scope-based injection, word-overlap injection fallback
- [x] Verify invariant: every scenario in at least one child after matching

## 4. Skill Documentation
<!-- specs: openspec-skill/spec -->
- [x] Update skills/openspec/SKILL.md with scenario-first grouping guidance
- [x] Add example showing spec-domain grouped tasks.md with `<!-- specs: ... -->` annotations
- [x] Update skills/cleave/SKILL.md to document annotation syntax and orphan behavior
