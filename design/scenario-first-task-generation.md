+++
id = "56100c98-30dd-4f8c-9b61-b86234d8a733"
kind = "design_node"
status = "implemented"
tags = ["cleave", "openspec", "task-generation"]
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
dependencies = []
open_questions = []
+++

# Scenario-First Task Generation

## Overview

When OpenSpec specs are decomposed into `tasks.md` for cleave children, the current approach groups tasks by **file/layer** (models → service → integration). This causes cross-cutting spec scenarios — particularly enforcement concerns like RBAC — to fall between children. No child's task description tells it to wire the enforcement logic, even though the spec clearly defines the scenario.

**Root cause:** `taskGroupsToChildPlans` in `openspec.ts` converts tasks.md groups verbatim. The generation step (LLM-produced tasks.md via `/opsx:ff`) organizes by file scope rather than by spec domain. The `buildDesignSection` scenario matcher in `workspace.ts` tries to recover by word-overlap heuristic, but this is too weak — it matches on shared words, not semantic intent.

**Fix:** Two-part solution:
1. **Scenario-first generation** — change task generation to group by spec domain rather than file layer
2. **Orphan detection with auto-inject** — after matching, any unmatched scenario is injected into the closest child with a `⚠️ cross-cutting` marker

## Research

### Current Flow

```
specs/*.md → /opsx:ff (LLM generates tasks.md) → parseTasksFile → taskGroupsToChildPlans → workspace.ts generateTaskFile → buildDesignSection (word-overlap scenario matching)
```

The scenario matching in `buildDesignSection` (workspace.ts:230-251):
- Splits child label+description and spec domain+requirement into words
- Matches if any word > 3 chars appears in both
- Filtered scenarios become "Acceptance Criteria" in the child task file

### Failure Mode (observed)

Spec `relay/rbac.md`: "Hub checks requester has `relay.request` before creating a session"

- Task group 2 (models): "Add relay.* capabilities to rbac.py" — got the *registration* but not *enforcement*
- Task group 3 (service): "create_session enforcing enabled check, global cap" — got *limits* but not *RBAC*
- Result: nobody wired `has_capability()` into `create_session()`

### Why File-Scoped Grouping Fails

Spec scenarios are behavioral contracts: Given precondition → When action → Then outcome. They naturally cross file boundaries. RBAC enforcement requires: (a) capability definitions in a model file, (b) a check function, (c) calling that check at the enforcement point in a service file. Splitting by file layer puts (a) and (c) in different children with no link.

## Decisions

### D1: Fix generation, not post-hoc remapping

Change the `/opsx:ff` plan phase to produce scenario-grouped tasks.md. Cleave executes what it's given. Human readability of tasks.md is secondary — correctness of child task descriptions is primary.

### D2: Auto-inject orphans with observability markers

After scenario-to-child matching, any scenario matching zero children is auto-injected into the closest child. A `⚠️ CROSS-CUTTING` marker is added for observability. No hard block — this is a safety net, not a gate.

### D3: Explicit spec-domain annotations in tasks.md

Each task group header carries a `<!-- specs: domain/name, ... -->` comment declaring which spec files it owns. This makes matching deterministic — no heuristic guessing. The generation step writes these because it knows which scenarios it grouped from.

### D4: Allow overlapping file scope

Scenario-first grouping will produce children that touch the same files for different reasons. This is acceptable. Cleave's existing merge conflict detection handles collisions. Forcing exclusive scope would re-introduce the layer-splitting problem.

## Open Questions

All resolved — see Decisions D5–D7.

### D5: Scenario-first generation via skill instructions

Add explicit grouping guidance to `skills/openspec/SKILL.md`: "When generating tasks.md, read all spec scenarios first. Group tasks so each group owns one or more spec domains end-to-end — from model changes through enforcement points. A group titled 'RBAC Enforcement' should include both 'add capability constants to rbac.py' AND 'call has_capability() in create_session()'. Don't split a spec domain's implementation across groups by architectural layer." This is a prompt/skill change — the LLM generating tasks.md needs better grouping instructions, not code changes.

### D6: Orphan injection target selection

First choice: the child whose file scope contains the enforcement file (deterministic — parse the scenario's When clause for the endpoint/function, find which child's scope includes that file). Fallback: highest word overlap with scenario text (heuristic, only for orphans where scope matching fails).

### D7: Spec-domain annotation syntax

HTML comment on the line after each task group header in tasks.md:
```markdown
## 2. RBAC Enforcement
<!-- specs: relay/rbac -->
```
The generation step writes these. The orphan detector reads them. Parseable by regex, invisible when rendered.

## Implementation Notes

### Affected Files

- `extensions/cleave/openspec.ts` — `taskGroupsToChildPlans`, scenario matching logic
- `extensions/cleave/workspace.ts` — `buildDesignSection`, orphan detection + auto-inject
- `skills/openspec/SKILL.md` — task generation guidance (scenario-first grouping)
- `skills/cleave/SKILL.md` — document new behavior
- `config/AGENTS.md` — update methodology section if needed

### Key Invariant

After all matching is complete, every spec scenario from the change's `specs/` directory must appear in at least one child's acceptance criteria. Zero-match = auto-inject + marker. This is verifiable in tests.
