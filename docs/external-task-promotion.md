---
id: external-task-promotion
title: "External task promotion into Omegon lifecycle"
status: exploring
tags: [acp, flynt, tasks, promotion, ecosystem]
parent: external-work-surface-integration
related: [acp-task-durability-contract, acp-task-binding-store, acp-task-mutation-contract]
open_questions:
  - "[assumption] Flynt can create arbitrary local tasks without Omegon involvement and those tasks remain Flynt-owned until explicit promotion."
  - "What offline artifact format should Flynt write when Omegon is unavailable?"
  - "Which lifecycle target should the online promotion flow default to: session plan, design node, OpenSpec change, or operator choice?"
  - "How should promoted Flynt body content be reviewed before becoming durable lifecycle text?"
---

# External task promotion into Omegon lifecycle

## Overview

Define how a task created in an external UI, initially Flynt, becomes Omegon lifecycle-backed without making the external UI a lifecycle authority. Flynt may create whatever local tasks its UX needs. Omegon authority begins only when the operator explicitly presses a promote/import action and Omegon either accepts a lifecycle mutation online or Flynt writes an offline promotion draft for later review.

This design preserves the authority boundary:

```text
Flynt-created task + no accepted promotion = Flynt-owned task.
Flynt-created task + offline promotion draft = pending review, not lifecycle truth.
Flynt-created task + Omegon accepted promotion = Omegon lifecycle task with Flynt binding.
```

## Promotion paths

### A. Offline / Omegon unavailable path

If no Omegon ACP session is available, Flynt may still prepare a promotion draft. It must not silently write OpenSpec/design lifecycle state as if Omegon accepted it.

Flynt should create a structured local artifact that:

- preserves the original Flynt task content;
- wraps it in an Omegon-promotion envelope;
- marks it `pending_review`;
- includes target hints if the operator selected them;
- is visible to Omegon/Flynt when online;
- does not claim repo-durable Omegon binding.

Suggested local payload shape:

```json
{
  "kind": "omegon_external_task_promotion_draft",
  "status": "pending_review",
  "system": "flynt",
  "external_task_id": "flynt-task-123",
  "created_at": "...",
  "target_hint": {
    "kind": "openspec|design|session|unknown",
    "change": "optional",
    "node_id": "optional"
  },
  "original": {
    "title": "Fix iCloud open-idle behavior",
    "body": "...",
    "board_id": "...",
    "column": "...",
    "tags": []
  },
  "review": {
    "required": true,
    "reason": "Omegon was unavailable when promotion was requested."
  }
}
```

Flynt can store this as either:

- a Flynt task body sub-element tagged `omegon-promotion-draft`;
- a project document under a Flynt-owned promotion queue;
- or both, if the document is the durable queue and the task embeds a pointer.

The key constraint: offline draft is an import request, not lifecycle state.

### B. Online / happy path

If Omegon ACP is available, Flynt should not mutate lifecycle files directly. Instead it should inject task context into the Flynt agent panel with a tailored prompt/skill that asks the agent to promote the Flynt task through Omegon's lifecycle authority.

The Flynt agent should receive:

- task id and URL/path;
- original title/body/subtasks;
- board/column/status context;
- existing `omegon-plan:<json>` refs, if any;
- target hints selected by the operator;
- current Omegon capability response;
- relevant `_plans/list` / `_tasks/list` projections;
- instruction to choose bind vs import vs design/OpenSpec creation.

Suggested agent prompt skeleton:

```text
Promote this Flynt-created task into Omegon lifecycle authority.

Rules:
- Treat the Flynt task as Flynt-owned until Omegon accepts a bind/import.
- First search existing Omegon plan/task projections for a matching lifecycle task.
- If a matching stable task exists, call `_tasks/bind` with requested_durability=repo and expected_revision.
- If no matching task exists, propose the correct lifecycle target: session plan, design node, or OpenSpec change.
- Do not directly edit lifecycle artifacts unless operating through Omegon-authorized tools/protocol.
- Preserve the original Flynt content as evidence/context.
- Return the accepted binding/import result or a review-needed reason.
```

## Decisions

### External GUI task creation remains local by default

**Status:** proposed

**Rationale:** Flynt should remain free to create visual/project tasks without requiring Omegon online availability. Local creation is not lifecycle mutation.

### Promotion requires explicit operator action

**Status:** proposed

**Rationale:** Promoting a Flynt task creates or links lifecycle state. That changes authority and must be deliberate.

### Offline promotion creates a review draft, not lifecycle truth

**Status:** proposed

**Rationale:** If Omegon is offline, Flynt cannot know the correct stable id, revision, target lifecycle artifact, or mutation policy. The safe action is to preserve intent and require review.

### Online promotion routes through a Flynt-agent skill

**Status:** proposed

**Rationale:** Promotion is contextual. A skill can inspect existing Omegon projections, decide bind vs import, and use capability negotiation without hard-coding brittle UI logic.

## Implementation Notes

Primary Flynt-side targets:

- Flynt task UI promote button.
- Flynt task body sub-element for `omegon-promotion-draft`.
- Flynt agent skill: `promote-flynt-task-to-omegon-lifecycle`.
- Flynt local link format: `omegon-plan:<json>`.

Primary Omegon-side targets:

- Future ACP method: `_external_tasks/import` or `_tasks/import`.
- Existing ACP methods: `_plans/list`, `_tasks/list`, `_tasks/bind`, `_runtime/capabilities`.
- Binding store: `.omegon/task-bindings.v1.json`.

Acceptance gates:

- Offline promotion never creates repo-durable Omegon binding.
- Online bind requires stable id and matching revision.
- Imported task response includes source, stable id, revision, and binding durability.
- Flynt UI shows states: local, promotion pending review, session-bound, repo-bound, conflict/stale.
