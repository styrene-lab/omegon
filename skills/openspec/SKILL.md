+++
id = "0c4d39b9-816f-4fe1-a8eb-a8337b6523c8"
name = "openspec"
description = "Spec-driven development lifecycle for non-trivial changes"
tags = []
aliases = ["opsx"]
activation = "lifecycle_gated"
profile = ["lifecycle"]
project_signals = ["openspec/changes", "openspec/baseline"]
+++

# OpenSpec — Spec-Driven Development Lifecycle

> **Load this skill** when working with OpenSpec changes, writing specs, generating tasks, or verifying implementations against specifications.

OpenSpec is a lifecycle-heavy skill. Use it only when the OpenSpec/lifecycle tool group is exposed or when the operator explicitly asks to work from OpenSpec files. If lifecycle tools are hidden, enable the relevant group with `manage_tools` before calling tool names such as `openspec_manage`, or operate directly on `openspec/changes/**` files and state that tool-backed lifecycle reconciliation was not performed.

## Overview

OpenSpec is Omegon's specification layer for spec-and-test-driven development. It ensures that every non-trivial change follows the lifecycle:

```
propose → specced → planned → testing → implementing → verifying → archived
```

Specs define **what must be true** before code is written. They are the source of truth for correctness.

## Lifecycle Stages

| Stage | Artifacts | Next Action |
|-------|-----------|-------------|
| **proposed** | `proposal.md` | Write specs with lifecycle tooling or by editing `specs/*.md` directly. |
| **specced** | `specs/*.md` | write `design.md` + `tasks.md`, then `openspec_manage(register_tasks)` |
| **planned** | `design.md`, `tasks.md` | register failing test stubs with `openspec_manage(register_test_file)` |
| **testing** | registered test stubs | implement until tests can pass |
| **implementing** | tasks in progress | Continue work directly or with cleave when exposed; update `tasks.md`, then register task progress when lifecycle tooling is available. |
| **verifying** | all tasks done | Assess scenarios against implementation, then archive with lifecycle tooling when available. |
| **archived** | specs merged to baseline | complete |

## Lifecycle Reconciliation (required)

OpenSpec artifacts are not write-once planning docs. Treat them as runtime lifecycle state.

At these checkpoints, reconcile the artifacts to match reality:

1. **Implement / scaffold** — ensure the design-tree node is bound to the OpenSpec change and marked `implementing`
2. **Post-plan / post-cleave** — ensure `tasks.md` reflects merged work, not just original intent, then register task progress with `openspec_manage` when lifecycle tooling is available
3. **Post-assess / post-fix** — after a spec or cleave assessment, reopen lifecycle state if review found remaining work, and append implementation-note deltas when fixes expanded file scope or constraints
4. **Pre-archive** — ensure the bound design-tree node and `tasks.md` are current before closing the change

Archive is expected to refuse obviously stale lifecycle state, especially:
- incomplete tasks in `tasks.md`
- no design-tree binding for the change
- missing registered test files before implementation

## Directory Structure

```
openspec/
├── changes/
│   └── <change-name>/
│       ├── proposal.md      # Intent, scope, success criteria
│       ├── design.md        # Architecture decisions, file changes
│       ├── tasks.md         # Numbered task groups for implementation/decomposition
│       └── specs/
│           ├── <domain>.md  # Delta specs with Given/When/Then
│           └── <domain>/
│               └── <sub>.md # Nested domain specs
├── baseline/                # Accumulated specs (post-archive)
│   └── <domain>.md
└── archive/                 # Completed changes (timestamped)
    └── YYYY-MM-DD-<name>/
```

## Spec File Format

Spec files use a **delta format** — they describe changes relative to the current baseline:

```markdown
# <domain> — Delta Spec

## ADDED Requirements

### Requirement: <title>

<description of what must be true>

#### Scenario: <scenario title>
Given <precondition>
When <action>
Then <expected outcome>
And <additional expectation>

## MODIFIED Requirements

### Requirement: <title>

<what changed and why>

#### Scenario: <updated scenario>
Given <new precondition>
When <action>
Then <updated expectation>

## REMOVED Requirements

### Requirement: <title>

<why this is being removed>
```

### Writing Good Scenarios

- **Given** establishes the starting state — be specific
- **When** is a single action — not a compound operation
- **Then** is the observable outcome — measurable and testable
- **And** adds additional assertions to Then

**Good:**
```
#### Scenario: Expired token rejected
Given a user has a JWT token that expired 5 minutes ago
When they make a GET request to /api/protected
Then the response status is 401
And the body contains {"error": "token_expired"}
```

**Bad:**
```
#### Scenario: Auth works
Given the system is running
When a user authenticates
Then it works correctly
```

### Deriving API Contracts from Scenarios

When a change introduces or modifies a network API (HTTP, gRPC, WebSocket), **derive an OpenAPI 3.1 spec** (or AsyncAPI for event-driven APIs) from the scenarios during the Plan phase. Place it at `openspec/changes/<id>/api.yaml`.

**Mapping rules:**

| Scenario element | OpenAPI element |
|------------------|-----------------|
| `Given` preconditions (auth, existing data) | Security schemes, parameter constraints, `x-setup` |
| `When ... request to <path>` | `paths.<path>.<method>`, request body schema |
| `Then status is <code>` | `responses.<code>` |
| `Then body contains {...}` | Response schema (`application/json`) |
| `And header <name> is <value>` | Response headers |
| Error scenarios (`401`, `404`, `422`) | Error response schemas, problem detail types |

**Example — from scenario to contract:**

Scenario:
```
Given a user has a valid API key
When they POST to /api/widgets with {"name": "foo", "color": "blue"}
Then the response status is 201
And the body contains {"id": "<uuid>", "name": "foo", "color": "blue"}
And the Location header contains /api/widgets/<uuid>
```

Derived OpenAPI fragment:
```yaml
paths:
  /api/widgets:
    post:
      security:
        - apiKey: []
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
              required: [name, color]
              properties:
                name: { type: string }
                color: { type: string }
      responses:
        '201':
          description: Widget created
          headers:
            Location:
              schema: { type: string, format: uri }
          content:
            application/json:
              schema:
                type: object
                properties:
                  id: { type: string, format: uuid }
                  name: { type: string }
                  color: { type: string }
```

The contract is the **source of truth for API shape** — code implements the contract. If implementation diverges, fix the code or amend the spec with rationale.

## Operator Commands and Tooling

OpenSpec may be driven through slash commands, registry commands, or direct tool calls depending on the current surface. Treat slash commands as operator-facing conveniences, not as mandatory agent instructions. Before invoking command-shaped flows, confirm the relevant command/tool is exposed; otherwise edit the OpenSpec files directly and state what lifecycle reconciliation remains.

Common operator command intents, when available:

| Intent | Typical command | Description |
|---------|-------------|-------------|
| Propose | `/opsx:propose <name> <title>` | Create a new change with proposal.md |
| Specify | `/opsx:spec <change>` | Generate/add specs |
| Fast-forward | `/opsx:ff <change>` | Scaffold design.md + tasks.md from specs, then register task progress |
| Status | `/opsx:status` | Show active changes with lifecycle stage |
| Verify | `/opsx:verify <change>` | Assess spec scenarios against implementation |
| Archive | `/opsx:archive <change>` | Archive change, merge specs to baseline |
| Apply | `/opsx:apply <change>` | Continue implementing, optionally via cleave when exposed |

## Tool: `openspec_manage`

Agent-callable tool for programmatic lifecycle operations.

### Actions

| Action | Required Params | Description |
|--------|----------------|-------------|
| `status` | — | List all active changes |
| `get` | `change_name` | Get change details, stage, spec summary |
| `propose` | `name`, `title`, `intent` | Create new change |
| `add_spec` | `change_name`, `domain`, `spec_content` | Add raw spec markdown |
| `register_tasks` | `change_name` | Read `tasks.md` and register task progress in the FSM |
| `register_test_file` | `change_name`, `path` | Register a failing test stub before implementation |
| `archive` | `change_name` | Archive completed change |

`register_tasks` reads counts from `tasks.md`; do not pass ad hoc task totals. Update OpenSpec first, then register.

## Integration with Cleave

OpenSpec and cleave work together when both lifecycle and cleave capabilities are exposed:

1. Fast-forward tooling or manual planning generates `tasks.md` in the format cleave expects (numbered groups with checkboxes).
2. Cleave uses `openspec/changes/<name>/tasks.md` as a split plan when that integration is available.
3. `cleave_run` may update task checkboxes on completion if its current schema exposes the OpenSpec change-path parameter.
4. `openspec_manage(register_tasks)` advances the FSM from the current `tasks.md` when lifecycle tooling is available.
5. A spec assessment verifies implementation against spec scenarios before archive.

### Scenario-First Task Grouping

When generating `tasks.md`, group tasks by **spec domain** — not by file layer. Each group should own the end-to-end implementation of one or more spec files, including all file changes needed to satisfy those scenarios, even if that means multiple groups touch the same file.

**Do:**
```markdown
## 1. RBAC Enforcement
<!-- specs: relay/rbac -->
- [ ] Add relay.request and relay.accept capabilities to rbac.py
- [ ] Wire has_capability() check into create_session() in relay_service.py
- [ ] Return 403 when capability missing
```

**Don't** (splits enforcement across layers):
```markdown
## 1. Models
- [ ] Add capabilities to rbac.py

## 2. Service Logic
- [ ] Add session limits to relay_service.py
(RBAC enforcement falls between chairs — nobody wires has_capability)
```

### Spec-Domain Annotations

Each task group header should include a `<!-- specs: domain/name -->` comment declaring which spec files the group owns. Multiple domains are comma-separated:

```markdown
## 2. Auth and Sessions
<!-- specs: relay/rbac, relay/session -->
- [ ] Implement auth checks
- [ ] Add session lifecycle
```

Cleave-capable workflows use these annotations to deterministically map spec scenarios to child tasks as acceptance criteria. Groups without annotations fall back to heuristic matching.

### tasks.md Format

The full format that cleave parses:
```markdown
## 1. Group Title
<!-- specs: domain/name -->

- [ ] 1.1 Task description
- [ ] 1.2 Another task
- [x] 1.3 Completed task
```

## Integration with Design Tree

The design-tree `implement` action scaffolds OpenSpec change directories from design nodes:

- Design node **children** → task groups
- Design node **decisions** → additional task groups
- Design node **open questions** → noted in tasks

### ⚠️ The scaffolder produces a draft — always rewrite tasks.md immediately

The scaffolder reads **decisions only**. It does NOT read research sections, impl_notes file scope, or constraints. The generated `tasks.md` will contain one vague one-liner per decision title. This is expected scaffolding behaviour — it is not a usable task list.

**Immediately after every `implement` call**, you must:

1. Read the generated `tasks.md`
2. Read the design node's `impl_notes` (file scope + constraints) and research sections
3. Rewrite `tasks.md` completely — treat the generated file as a placeholder, not a draft to polish

**What a correct rewrite looks like:**

- One task group per file or coherent feature area (derived from impl_notes file scope)
- Each constraint maps to at least one concrete task item
- Research code examples (method signatures, class names) translate into numbered implementation tasks
- Rejected decisions are omitted entirely — never "implement" a rejected decision
- Dependencies between groups are stated explicitly at the top of the file
- If a scaffolded OpenSpec was created before a design decision was superseded, the tasks must reflect the current decision, not the old one

**Detecting a bad tasks.md:**

- Any task item whose text is a verbatim copy of a decision title → rewrite required
- Any task group labelled "Implement [rejected decision]" → immediately rewrite
- Fewer than 3 concrete numbered subtasks per group → likely too shallow
- No mention of specific method names, file paths, or test assertions → too abstract

This rewrite step is not optional polish — it is the primary authoring step for task content. The scaffolder provides structure; the agent provides substance.

## When to Use OpenSpec

**Use OpenSpec for:**
- Multi-file changes, especially when `cleave_assess` is available and reports complexity ≥ 2.0 and the work naturally splits into 2+ coordinated child scopes
- Any change affecting public APIs or data models
- Cross-cutting concerns (auth, logging, error handling)
- Changes that will be reviewed by others

**Skip for:**
- Single-file fixes, typos, config tweaks
- Changes with obvious correctness (renaming, formatting)
- Urgent hotfixes (document retroactively)

## Workflow Example

Capability-aware workflow:

```text
1. Propose: create openspec/changes/jwt-auth/proposal.md with intent and success criteria.
2. Specify: write delta specs with Given/When/Then scenarios under specs/.
3. Plan: write design.md and scenario-owned task groups in tasks.md.
4. Register: if openspec_manage is exposed, register tasks and test files.
5. Implement: work directly, use `delegate` for bounded one-shot side quests (scout/patch/verify), or use `cleave_run` when there are 2+ coordinated child scopes that benefit from worktree isolation and merge governance.
6. Verify: assess implementation against scenarios; fix or amend specs with rationale.
7. Archive: use lifecycle tooling when available, otherwise document remaining reconciliation.
```

Slash commands such as `/opsx:propose`, `/opsx:spec`, `/cleave`, and `/assess spec` are examples of one operator surface. Do not assume they exist in every session or provider mode.
