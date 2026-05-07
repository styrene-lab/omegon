+++
id = "54e9c42f-bd83-4fd9-a53c-eb93bec520bc"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Design: Skill-Aware Child Dispatch

## Architecture Decisions

### D1: Hybrid skill matching
Auto-match from scope file patterns (*.py→python, *.rs→rust, Containerfile→oci) as default. `<!-- skills: python, k8s-operations -->` annotation in tasks.md overrides/augments. Annotation takes precedence — if present, auto-match is skipped for that child.

### D2: Directive injection (not inline)
Child prompt gets "Before starting, read these skill files: ..." with paths. Skills are 200+ lines — inlining would bloat prompts. Child already has `read` tool access.

### D3: Extensible mapping table
`SkillMapping[]` data structure with glob patterns → skill name + optional tier hint. Default mappings for common languages/tools. Projects can extend via future config.

### D4: Tiered execution loop
```
Execute (cheap) → Review (opus) → [pass? done : Fix (cheap) → Review (opus)]
```
- Execute model: local/haiku/sonnet based on skill hints + scope
- Review model: always opus/thinking tier (highest available)
- Fix model: same as execute model

### D4a: Severity-gated escalation
- Nits only → pass, no fix
- Warnings → 1 fix iteration max
- Critical → 2 fix iterations, then escalate
- Critical+security → immediate escalate, no fix attempt

### D4b: Diminishing returns guardrail
- Hash issue descriptions between rounds
- >50% reappearance (Jaccard similarity) → bail, report to orchestrator
- Catches cheap-model-going-in-circles degenerate case

### D4c: Adversarial review posture
Review agent is hostile. Checks: spec scenario satisfaction, bugs, security, omissions, scope compliance. Output: PASS/FAIL verdict + categorized issues (C/W/N).

### D5: Review in same worktree
Review agent runs in the child's worktree with full file access. Needs to read files for context, not just scan diffs.

### D6: Parallel reviews
Reviews run in parallel across children, same as execution. Each review is independent.

## File Scope

| File | Changes |
|------|---------|
| `extensions/cleave/skills.ts` | New: `SkillMapping`, `DEFAULT_MAPPINGS`, `matchSkillsToChild()`, `resolveSkillPaths()` |
| `extensions/cleave/review.ts` | New: `ReviewVerdict`, `ReviewIssue`, `buildReviewPrompt()`, `parseReviewResult()`, `buildFixPrompt()`, `detectChurn()`, `executeWithReview()` |
| `extensions/cleave/dispatcher.ts` | Modified: `dispatchSingleChild` calls `executeWithReview`, accepts review config, passes model per child |
| `extensions/cleave/workspace.ts` | Modified: `buildChildPrompt` accepts skill paths, emits skill read directives |
| `extensions/cleave/types.ts` | Modified: `ChildPlan.skills: string[]`, `ChildPlan.executeModel?`, `ChildState.reviewIterations?`, review-related types |
| `extensions/cleave/openspec.ts` | Modified: parse `<!-- skills: ... -->` annotations alongside `<!-- specs: ... -->` |
| `extensions/cleave/index.ts` | Modified: `cleave_run` tool accepts `review` and `skill_matching` config options |
| `skills/cleave/SKILL.md` | Modified: document skill annotations, review loop, model tier routing |
