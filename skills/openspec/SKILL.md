---
name: openspec
description: OpenSpec lifecycle for spec-driven development. Use when proposing changes, writing Given/When/Then specs, generating tasks, verifying implementations, or archiving completed changes. Commands /opsx:propose, /opsx:spec, /opsx:ff, /opsx:status, /opsx:verify, /opsx:archive.
---

# OpenSpec — Spec-Driven Development Lifecycle

> **Load this skill** when working with OpenSpec changes, writing specs, generating tasks, or verifying implementations against specifications.

## Overview

OpenSpec is pi-kit's specification layer for spec-and-test-driven development. It ensures that every non-trivial change follows the lifecycle:

```
propose → spec → plan → implement → verify → archive
```

Specs define **what must be true** before code is written. They are the source of truth for correctness.

## Lifecycle Stages

| Stage | Artifacts | Next Action |
|-------|-----------|-------------|
| **proposed** | `proposal.md` | `/opsx:spec <change>` — write specs |
| **specified** | `specs/*.md` | `/opsx:ff <change>` — generate design + tasks |
| **planned** | `design.md`, `tasks.md` | `/cleave` — execute tasks |
| **implementing** | tasks in progress | continue work or `/cleave` |
| **verifying** | all tasks done | `/assess spec <change>` → `/opsx:archive` |
| **archived** | specs merged to baseline | complete |

## Lifecycle Reconciliation (required)

OpenSpec artifacts are not write-once planning docs. Treat them as runtime lifecycle state.

At these checkpoints, reconcile the artifacts to match reality:

1. **Implement / scaffold** — ensure the design-tree node is bound to the OpenSpec change and marked `implementing`
2. **Post-cleave** — ensure `tasks.md` reflects merged work, not just original intent
3. **Post-assess / post-fix** — after `/assess spec` or `/assess cleave`, reopen lifecycle state if review found remaining work, and append implementation-note deltas when fixes expanded file scope or constraints
4. **Pre-archive** — ensure the bound design-tree node and `tasks.md` are current before closing the change

Archive is expected to refuse obviously stale lifecycle state, especially:
- incomplete tasks in `tasks.md`
- no design-tree binding for the change

## Directory Structure

```
openspec/
├── changes/
│   └── <change-name>/
│       ├── proposal.md      # Intent, scope, success criteria
│       ├── design.md        # Architecture decisions, file changes
│       ├── tasks.md         # Numbered task groups for /cleave
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

## Commands

| Command | Description |
|---------|-------------|
| `/opsx:propose <name> <title>` | Create a new change with proposal.md |
| `/opsx:spec <change>` | Generate/add specs (triggers agent to write scenarios) |
| `/opsx:ff <change>` | Fast-forward: scaffold design.md + tasks.md from specs |
| `/opsx:status` | Show all active changes with lifecycle stage |
| `/opsx:verify <change>` | Delegates to `/assess spec` for spec verification |
| `/opsx:archive <change>` | Archive change, merge specs to baseline |
| `/opsx:apply <change>` | Continue implementing (delegates to `/cleave`) |

## Tool: `openspec_manage`

Agent-callable tool for programmatic lifecycle operations.

### Actions

| Action | Required Params | Description |
|--------|----------------|-------------|
| `status` | — | List all active changes |
| `get` | `change_name` | Get change details, stage, spec summary |
| `propose` | `name`, `title`, `intent` | Create new change |
| `add_spec` | `change_name`, `domain`, `spec_content` | Add raw spec markdown |
| `generate_spec` | `change_name`, `domain` | Generate spec scaffold from proposal |
| `fast_forward` | `change_name` | Generate design.md + tasks.md |
| `archive` | `change_name` | Archive completed change |

### `generate_spec` Optional Params

- `decisions`: Array of `{title, rationale}` — generates requirements per decision
- `open_questions`: Array of strings — generates MODIFIED Requirements placeholders

## Integration with Cleave

OpenSpec and cleave work together:

1. **`/opsx:ff`** generates `tasks.md` in the format cleave expects (numbered groups with checkboxes)
2. **`/cleave`** detects `openspec/changes/<name>/tasks.md` and uses it as the split plan
3. **`cleave_run`** with `openspec_change_path` updates task checkboxes on completion
4. **`/assess spec`** verifies implementation against spec scenarios

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

Cleave uses these annotations to deterministically map spec scenarios to child tasks as acceptance criteria. Groups without annotations fall back to heuristic matching.

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

**Always use** for:
- Multi-file changes (complexity ≥ 2.0 in cleave_assess)
- Any change affecting public APIs or data models
- Cross-cutting concerns (auth, logging, error handling)
- Changes that will be reviewed by others

**Skip for:**
- Single-file fixes, typos, config tweaks
- Changes with obvious correctness (renaming, formatting)
- Urgent hotfixes (document retroactively)

## Workflow Example

```
# 1. Propose
/opsx:propose jwt-auth "JWT Authentication"

# 2. Write specs (agent generates Given/When/Then)
/opsx:spec jwt-auth

# 3. Generate implementation plan
/opsx:ff jwt-auth

# 4. Execute in parallel
/cleave implement jwt-auth changes

# 5. Verify against specs
/assess spec jwt-auth

# 6. Archive
/opsx:archive jwt-auth
```
