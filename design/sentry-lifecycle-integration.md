# Sentry + Lifecycle + OpenSpec Integration

## Context

Sentry executes tasks autonomously. The design tree tracks what needs building. OpenSpec defines acceptance criteria. These three systems should close a loop:

```
Design tree (what to build)
  → Sentry (build it)
    → OpenSpec (verify it)
      → Design tree (mark it done)
```

Today these are disconnected — sentry tasks are prompt strings in a TOML file, design nodes are markdown files with status frontmatter, and OpenSpec scenarios are documentation. This spec connects them.

## What Already Works

The infrastructure is further along than expected:

| Capability | Status | Code Path |
|---|---|---|
| Design node status mutation | Working | `design_tree_update` tool → FSM validation → markdown write |
| Node discovery (what's ready) | Working | `workflow::query_ready_nodes()` filters `status=Decided` + dependencies satisfied |
| Prompt generation from nodes | Working | `workflow::build_dispatch_prompt()` |
| Context injection (focused node) | Working | `LifecycleContextProvider::provide_context()` auto-injects focused node + active changes |
| OpenSpec stage computation | Working | `spec::compute_stage()` derives stage from task checkbox counts |
| Scenario parsing | Working | `spec.rs` parses Given/When/Then into `Scenario` structs |

## What's Missing

Three gaps block the closed loop:

### Gap 1: tasks.md Mutation

OpenSpec stages advance when checkbox counts change in `tasks.md`. There's no tool to safely mark a task done. The agent would need to use `write` or `bash` to edit the file — fragile and unprincipled.

**Fix:** Add `action="mark_task"` to `openspec_manage` tool.

```rust
// In lifecycle.rs, openspec_manage handler:
"mark_task" => {
    let change = args.get("change").ok_or("missing change name")?;
    let task_index = args.get("task_index").ok_or("missing task_index")?;
    let done: bool = args.get("done").unwrap_or("true").parse()?;
    openspec_mark_task(change, task_index, done)?;
    // compute_stage() will automatically reflect the new count
}
```

This is the smallest gap — ~30 lines of code.

### Gap 2: Design-Aware Task Discovery

Sentry can only execute tasks defined in `sentry.toml` (FileTaskBoard) or Flynt boards (future). It can't discover tasks from the design tree. There should be a mode where sentry watches the design tree and auto-creates tasks for nodes that reach "Decided" status with all dependencies satisfied.

**Fix:** Add `DesignTreeTaskBoard` — a third `TaskBoard` implementation.

```rust
pub struct DesignTreeTaskBoard {
    cwd: PathBuf,
    state_db: Arc<StateDb>,
    instance_id: String,
}

impl TaskBoard for DesignTreeTaskBoard {
    fn list_actionable(&self) -> Result<Vec<SentryTask>> {
        let ready_nodes = workflow::query_ready_nodes(&self.cwd);
        ready_nodes.iter().map(|node| SentryTask {
            id: node.id.clone(),
            name: node.title.clone(),
            priority: node.priority_as_u8(),
            triggers: vec![Trigger::Manual],  // discovered on each tick
            last_run: self.state_db.last_run(&node.id)?.map(|(dt, _)| dt),
            run_count: self.state_db.last_run(&node.id)?.map(|(_, c)| c).unwrap_or(0),
        }).collect()
    }

    fn task_spec(&self, task_id: &str) -> Result<TaskSpec> {
        let node = workflow::find_node(&self.cwd, task_id)?;
        Ok(TaskSpec {
            prompt: workflow::build_dispatch_prompt(&node),
            model: None,  // uses sentry default
            ..Default::default()
        })
    }
    // claim/release/complete/fail delegate to state_db
}
```

This is a medium effort — ~100 lines, reuses existing `workflow::` functions.

### Gap 3: Scenario Verification Mode

Sentry should be able to run an agent specifically to verify OpenSpec scenarios. Not freeform "check if it works" — structured verification where each Given/When/Then is checked and the result is recorded.

**Fix:** Add a `verify` mode to sentry tasks.

When a sentry task has `skill = "verify"` (or a dedicated config field), the executor:
1. Reads the OpenSpec change associated with the task
2. Extracts all scenarios from specs
3. Builds a structured verification prompt
4. Runs the agent with scenario-aware context
5. On success: marks all tasks.md items as done → stage auto-advances to "Verifying"

The verification prompt template:

```
You are verifying that the implementation of "{change_name}" satisfies its
acceptance criteria.

For EACH scenario below, you must:
1. Set up the precondition (Given)
2. Execute the action (When)
3. Assert the expected outcome (Then)
4. Report PASS or FAIL with evidence

Scenarios:
{for each spec.requirement.scenario}
## {scenario.title}
- **Given** {scenario.given}
- **When** {scenario.when}
- **Then** {scenario.then}
{end for}

After verifying all scenarios, summarize: how many passed, how many failed,
and any issues found. If ALL pass, report VERIFIED.
```

This is the largest gap — ~150 lines of executor logic + prompt template.

## Integration Architecture

### TaskSpec Extension

Add optional lifecycle metadata to `TaskSpec`:

```rust
pub struct TaskSpec {
    // ... existing fields ...
    pub design_node_id: Option<String>,
    pub openspec_change: Option<String>,
    pub verification_mode: bool,
}
```

When `design_node_id` is set:
- Executor calls `design_tree_update(action="focus", node_id)` before running
- On success: calls `design_tree_update(action="set_status", node_id, "implemented")`
- Context injection automatically shows the focused node's details

When `openspec_change` is set:
- Executor loads scenarios from the change
- Injects scenario context into the prompt
- On success: marks tasks done via `openspec_manage(action="mark_task")`

When `verification_mode` is true:
- Executor builds the structured verification prompt instead of using `spec.prompt`
- Requires `openspec_change` to be set

### sentry.toml Extension

```toml
[[task]]
name = "implement-auth-rewrite"
prompt = "Implement the auth rewrite as specified in the design node"
model = "anthropic:claude-sonnet-4-6"
max_turns = 50
design_node_id = "auth-rewrite-2026"

[task.trigger.webhook]
name = "implement"

[[task]]
name = "verify-auth-rewrite"
prompt_file = "tasks/verify-auth.md"
model = "anthropic:claude-sonnet-4-6"
max_turns = 30
openspec_change = "auth-rewrite"
verification_mode = true

[task.trigger.webhook]
name = "verify"
```

### Lifecycle Event Flow

When a sentry task completes with `design_node_id` set:

```
Task completes (exit_code=0)
  → executor calls design_tree_update(set_status="implemented")
  → FSM validates transition
  → Markdown frontmatter updated
  → If openspec_change set:
    → Mark tasks done in tasks.md
    → compute_stage() → "Verifying"
  → Next tick: query_ready_nodes() discovers downstream nodes
  → Dependent nodes now eligible for execution
```

This creates a cascade: completing one node unblocks the next, and sentry picks it up on the next evaluation tick.

### DesignTreeTaskBoard Auto-Discovery

With `DesignTreeTaskBoard`, the workflow becomes fully autonomous:

```
Developer marks design node "Decided"
  → Sentry tick: query_ready_nodes() finds it
  → build_dispatch_prompt() generates task prompt
  → Sentry executes: agent implements the node
  → On completion: node → "Implemented"
  → Downstream nodes become ready
  → Next tick: sentry picks up the next node
  → Cascade until all decided nodes are implemented
```

This requires no TOML config — the design tree IS the task board.

## Implementation Phases

### Phase A: tasks.md Mutation Tool

Add `mark_task` action to `openspec_manage`. ~30 lines.

- Parse tasks.md, find task by index or text match
- Toggle checkbox (`- [ ]` ↔ `- [x]`)
- Write back
- Stage advances automatically on next `compute_stage()` call

### Phase B: TaskSpec Lifecycle Metadata

Extend `TaskSpec` with `design_node_id`, `openspec_change`, `verification_mode`. Wire executor to:
- Focus the design node before task execution
- Update design node status on completion
- Mark openspec tasks on completion

Extend `sentry.toml` parser and `FileTaskBoard` to expose these fields.

~60 lines across executor.rs, types.rs, mod.rs.

### Phase C: Verification Mode

Build the structured verification prompt template. When `verification_mode=true`:
- Load scenarios from the named openspec change
- Build the verification prompt
- Override `spec.prompt` with the generated prompt

~100 lines in executor.rs + a prompt template.

### Phase D: DesignTreeTaskBoard

New `TaskBoard` implementation that discovers tasks from `query_ready_nodes()`. This is the fully autonomous mode — design tree drives execution without explicit task config.

~100 lines in a new `design_board.rs`.

### Phase E: Lifecycle Events

Add `SentryTaskCompleted { task_id, design_node_id, exit_code }` to `BusEvent`. Emit from executor on completion. LifecycleFeature can listen for this and trigger exports, memory facts, or notifications.

~40 lines across bus types + lifecycle feature.

## Non-Goals

- **Sentry does not replace the agent loop's design tree tools.** The agent inside a sentry task has full access to `design_tree_update`, `openspec_manage`, etc. Sentry's lifecycle integration is about orchestration (what to run, when, and how to record results), not about replacing tool-level functionality.
- **Sentry does not own the design tree state machine.** The FSM in omegon-opsx validates transitions. Sentry proposes transitions; the FSM enforces correctness.
- **Scenario verification is agent-driven, not programmatic.** The agent interprets Given/When/Then in natural language and uses tools to verify. There's no BDD test runner — the LLM IS the test runner.

## Open Questions

1. **Should DesignTreeTaskBoard be the default?** If the project has a design tree, should sentry auto-discover from it without explicit config? Or should it require opt-in (`--design-tree` flag)?

2. **Verification confidence.** An agent saying "VERIFIED" isn't the same as a test suite passing. Should there be a review step where a human confirms the verification before stage advances?

3. **Cascade depth.** If completing node A unblocks B, C, and D, should sentry execute all three in sequence on the same tick? Or stagger across ticks? The stagger delay helps, but deep cascades could still be aggressive.
