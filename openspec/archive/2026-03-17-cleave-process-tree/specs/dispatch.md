+++
id = "64be5dac-38d4-4b26-862c-e4dea257899f"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# dispatch — Delta Spec

## ADDED Requirements

### Requirement: spawnChild uses RPC mode for structured bidirectional communication

#### Scenario: Child process is spawned in RPC mode with prompt command

Given a cleave dispatch with a task file and resolved model tier
When spawnChild is called
Then the child process is spawned with `--mode rpc --no-session` instead of `-p --no-session`
And the task prompt is sent as a JSON `{type: "prompt", message: ...}` command on stdin
And stdin remains open for the session lifetime (not closed after prompt)

#### Scenario: Structured agent events replace stdout line scraping

Given a child process running in RPC mode
When the child emits agent events (tool_call, message_end, etc.)
Then the parent parses JSON lines from stdout
And each event is typed (has a `type` field matching AgentSessionEvent)
And progress updates are derived from event types, not from heuristic line filtering
And the `isChildStatusLine` and `stripAnsiForStatus` functions are no longer used for RPC children

#### Scenario: Child completion is detected from RPC events and process exit

Given a child process running in RPC mode
When the child completes its task
Then the parent observes the final `message_end` event
And the parent reads the task file for Status/Summary/Artifacts (unchanged contract)
And the child process exits normally

### Requirement: Task file contract is preserved

#### Scenario: RPC children still write standard task files

Given a child process running in RPC mode
When the child completes work
Then the task file `N-task.md` contains Status, Summary, Artifacts, Decisions Made, Interfaces Published
And the merge process reads the task file identically to pipe-mode children
And conflict detection in `conflicts.ts` works without modification

### Requirement: Review subprocess stays on pipe mode in Phase 1

#### Scenario: Review spawns use pipe mode regardless of execution mode

Given a cleave dispatch with review enabled
When the review subprocess is spawned (via ReviewExecutor.review)
Then the review process uses `-p --no-session` (pipe mode)
And the review result is parsed from stdout text as before
And the execution subprocess uses `--mode rpc --no-session`

### Requirement: Graceful degradation when RPC pipe breaks

#### Scenario: Child continues working if parent-to-child stdin closes

Given a child process running in RPC mode
And the parent process crashes or the stdin pipe breaks
When the child's stdin receives EOF
Then the child continues executing with its existing task prompt
And the child writes results to the task file
And the child's git branch preserves all committed work
And the parent (on recovery) can read the task file and merge the branch

#### Scenario: Parent handles child stdout closing unexpectedly

Given a child process running in RPC mode
When the child's stdout closes before process exit (pipe break)
Then the parent treats the child as failed with a pipe-break error
And the child's worktree and branch are preserved (not cleaned up)
And the dispatch continues with remaining children in the wave

### Requirement: Dashboard progress uses structured events

#### Scenario: emitCleaveChildProgress consumes typed events

Given a child process running in RPC mode
When the child emits a tool_call event
Then the parent extracts the tool name and emits a progress update
And the dashboard shows structured status (e.g., "tool: read src/auth.ts") instead of scraped text

#### Scenario: Progress updates are not debounced for RPC children

Given a child process running in RPC mode
When agent events arrive on stdout
Then events are parsed and forwarded without the 500ms debounce timer
And the debounced onChildLine callback is not used for RPC children
