# Lifecycle Work Plan Threading — Delta Spec

## ADDED Requirements

### Requirement: Ephemeral plans remain lightweight

Small work plans may exist without a design-tree node or OpenSpec change binding.

#### Scenario: Create an ephemeral plan
Given no design-tree node or OpenSpec change is focused
When the operator creates a small work plan
Then the plan is marked as ephemeral
And the plan can be edited and completed within the current conversation
And no OpenSpec task file is created implicitly

### Requirement: Bound plans disclose their source

The visible plan UX must show whether the plan is ephemeral, design-bound, OpenSpec-bound, or hybrid.

#### Scenario: Display OpenSpec-backed tasklist
Given an OpenSpec change with a task file is focused
When the small tasklist is rendered
Then the tasklist shows that its source is OpenSpec
And the rendered items identify the backing change name
And any write-through action is distinguishable from a runtime-only action

### Requirement: Plan state has a single completion authority

Plan completion state must not diverge between item state and plan mode.

#### Scenario: Complete final task item
Given a work plan with one remaining incomplete item
When that item is marked complete
Then the plan completion state is updated atomically
And later snapshots do not show the plan as both active and complete

### Requirement: Plan mutations use one central action API

Slash commands, tool calls, and future registry operations must mutate plan state through the same action boundary.

#### Scenario: Slash and tool completion follow same mutation path
Given a visible session plan with an active item
When the item is completed through a slash command
And another item is completed through a plan tool call
Then both updates use the same plan action semantics
And item state, plan mode, snapshots, and completed history remain consistent

### Requirement: Legacy session snapshots migrate to visible plan state

Existing session snapshots with legacy work plan fields must resume without losing plan state.

#### Scenario: Resume legacy snapshot
Given a saved session snapshot contains `work_plan` and `plan_mode` but no visible plan wrapper
When the snapshot is loaded
Then the legacy fields are normalized into an ephemeral session visible plan
And the backward-compatible snapshot projection still contains mode, completed, total, and items

### Requirement: Clearing visible plans is non-destructive by default

Clearing the visible tasklist must not silently delete durable lifecycle state.

#### Scenario: Clear OpenSpec-bound visible plan
Given the visible plan is projected from an OpenSpec task file
When the operator clears the visible plan
Then the OpenSpec task file remains unchanged
And the UI reports that only the runtime view was cleared or detached

### Requirement: Write-through to lifecycle artifacts is explicit

Actions that mutate OpenSpec or design-tree state from the small plan must be explicit and auditable.

#### Scenario: Complete OpenSpec-backed item
Given a visible plan item is backed by an OpenSpec task line
When the operator marks the item complete with write-through enabled
Then the corresponding task line in the task file is updated
And the lifecycle read model reflects the updated task count
And the operator can see that durable lifecycle state changed

### Requirement: OpenSpec write-through requires stable task identity

The system must not write to an OpenSpec task file unless the target task identity is stable enough to avoid updating the wrong checkbox.

#### Scenario: OpenSpec task lacks stable identity
Given a visible plan item is projected from an OpenSpec checkbox with no stable task identity
When the operator requests write-through completion
Then the system refuses or falls back to runtime-only completion
And explains that stable task identity is required before durable write-through

### Requirement: Plans are organized through a registry

The system must track active, backgrounded, blocked, completed, detached, and archived plans through a registry keyed by stable plan identity.

#### Scenario: Background plan remains tracked while hidden
Given a plan is backgrounded while another plan is visible
When a background task item changes state
Then the registry updates the backgrounded plan progress
And the visible plan remains unchanged
And the operator can list or switch back to the backgrounded plan

### Requirement: Background completions do not steal focus

A plan that completes while hidden must not replace the foreground tasklist.

#### Scenario: Hidden plan completes
Given the operator is viewing plan A
And plan B is backgrounded
When plan B completes
Then plan A remains the visible plan
And the UI emits a concise completion notification for plan B
And plan B appears in the completed or recent plan lane

### Requirement: Completed plans are preserved as evidence

Completed plans must be recorded in a completion ledger with lifecycle and evidence references when available.

#### Scenario: Record completed lifecycle-bound plan
Given an OpenSpec-bound plan completes
When the completion is recorded
Then the ledger includes the plan id, title, source, binding, completion time, item count, and summary
And the ledger can reference commits, validations, design nodes, and OpenSpec changes when those references exist

### Requirement: Resume uses ranked plan candidates

Session resume must be explicit but assisted through ranked lifecycle-aware candidates.

#### Scenario: Resume after multiple active workstreams
Given the previous session had an active foreground plan
And another lifecycle-bound plan was backgrounded with recent activity
And a third plan completed recently
When the operator resumes work
Then the system presents ranked resume candidates
And incomplete active or blocked plans rank above completed context
And no plan is silently made active without operator selection

### Requirement: Plans distinguish session scope from repo scope

Plans must indicate whether they are session-scoped runtime checklists or repo-bound lifecycle projections.

#### Scenario: Promote session plan to repo-bound work
Given a session-scoped plan has become multi-session or backgrounded
When the operator chooses to promote the plan
Then the plan becomes bound to a design node, OpenSpec change, branch, or hybrid lifecycle target
And the UI shows the new repo-bound source
And clearing the visible plan detaches the projection instead of deleting durable lifecycle state

### Requirement: Non-coding task intents are first-class

Plans must support research, design, writing, review, operations, validation, and decision capture tasks without forcing them into implementation-only semantics.

#### Scenario: Track research task completion
Given a plan item has research intent
When the item is completed
Then the completion evidence records findings or citations rather than requiring a code diff
And the plan can bind the evidence to a design node research section

#### Scenario: Track design task completion
Given a plan item has design intent
When the item is completed
Then the completion evidence records decisions, resolved questions, or updated design artifacts
And the plan remains eligible for later promotion to OpenSpec if acceptance criteria emerge

### Requirement: Plan commands expose list, switch, resume, detach, promote, and ledger operations

Operators must be able to manage multiple foreground, backgrounded, completed, and repo-bound plans explicitly.

#### Scenario: List plans across foreground and background work
Given one session plan is visible
And one OpenSpec-bound plan is backgrounded
And one plan completed recently
When the operator lists plans
Then the response includes each plan id, title, source, scope, status, progress, and resume hint when available

#### Scenario: Detach repo-bound plan
Given an OpenSpec-bound plan is visible
When the operator detaches the plan
Then the visible plan is cleared
And the plan registry marks the plan detached or backgrounded according to policy
And the backing OpenSpec task file remains unchanged

#### Scenario: Promote session plan to OpenSpec work tracking
Given a session-scoped plan has durable work-tracking value
When the operator promotes it to OpenSpec
Then an OpenSpec change or task group is created or selected
And the plan binding records the OpenSpec change
And future task completion can record OpenSpec-backed evidence

### Requirement: Completion evidence matches task intent

The completion ledger must support different evidence types for different task intents.

#### Scenario: Complete operations task
Given a plan item has operations intent
When the item is completed
Then the ledger can reference branch, worktree, tag, deployment, or remote state evidence
And no code diff is required for the item to be considered complete

#### Scenario: Complete validation task
Given a plan item has validation intent
When the item is completed
Then the ledger records test, lint, smoke, or assessment evidence
And the item can block completion if required validation failed

### Requirement: Task completion policies define done semantics

Task intent labels must be paired with completion policies when evidence requirements matter.

#### Scenario: Research task requires findings
Given a research task has an evidence-required completion policy
When the operator tries to mark it complete without findings or citations
Then the system reports the missing evidence
And the task remains incomplete unless the operator explicitly overrides with a manual completion policy

#### Scenario: Design task resolves an open question
Given a design task completion policy requires a decision or resolved question
When the task completes with a recorded design decision
Then the completion evidence links to that decision
And the related open question can be marked resolved