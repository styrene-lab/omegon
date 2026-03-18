# progress — Delta Spec

## ADDED Requirements

### Requirement: Rust orchestrator emits NDJSON progress events on stdout

#### Scenario: Child lifecycle events appear on stdout as JSON

Given a cleave run with 2 children in one wave
When the Rust orchestrator dispatches children
Then stdout contains a `wave_start` event with both child labels
And stdout contains a `child_spawned` event for each child with pid
And stdout contains a `child_status` event with status `completed` or `failed` for each child
And each JSON line is valid self-contained JSON (parseable independently)

#### Scenario: Merge phase events appear on stdout

Given a cleave run where all children complete
When the orchestrator enters the merge phase
Then stdout contains a `merge_start` event
And stdout contains a `merge_result` event for each child with success boolean
And stdout contains a `done` event with completed/failed counts and total duration

#### Scenario: No non-JSON output on stdout

Given a cleave run
When the orchestrator runs to completion
Then every line on stdout is valid JSON (no tracing output, no bare text)
And all tracing/diagnostic output remains on stderr only

### Requirement: Child activity events from agent tool calls

#### Scenario: Tool calls are relayed as child_activity events

Given a child agent that calls write and bash tools
When the orchestrator sees tool-call lines in child stderr
Then stdout contains `child_activity` events with tool name and target
And activity events are throttled to at most 1 per second per child

#### Scenario: Turn boundaries are relayed

Given a child agent that runs for 3 turns
When the orchestrator sees turn markers in child stderr
Then stdout contains `child_activity` events with the turn number

### Requirement: TS wrapper maps progress events to dashboard state

#### Scenario: Dashboard shows running children during dispatch

Given a cleave_run invocation via the tool interface
When the Rust orchestrator emits child_spawned events
Then sharedState.cleave.children[i].status becomes "running"
And sharedState.cleave.children[i].startedAt is set

#### Scenario: Dashboard shows activity lines for running children

Given a running cleave child emitting tool calls
When child_activity events arrive
Then sharedState.cleave.children[i].lastLine is updated with the activity summary
And the dashboard footer shows the activity under the child's row
