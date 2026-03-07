# Shared State Data Contract

The dashboard relies on a well-defined shared state interface that each producer extension writes to and the dashboard extension reads from.

## Requirements

### R1: SharedState extends with dashboard types
The SharedState interface in shared-state.ts must include optional properties for designTree, openspec, and cleave dashboard state.

### R2: Design tree state shape
The designTree state must include: nodeCount, decidedCount, exploringCount, blockedCount, openQuestionCount, and focusedNode (nullable object with id, title, status, questions array).

### R3: OpenSpec state shape
The openspec state must include a changes array where each entry has: name, stage, tasksDone, tasksTotal.

### R4: Cleave state shape
The cleave state must include: status (idle|assessing|planning|dispatching|merging|done|failed), optional runId, and optional children array where each child has: label, status (pending|running|done|failed), optional elapsed seconds.

## Scenarios

### S1: SharedState interface compiles with all dashboard types
Given shared-state.ts is imported
When TypeScript compiles the SharedState interface
Then it includes designTree?, openspec?, and cleave? properties with correct types
And all properties are optional (existing consumers unaffected)

### S2: SharedState defaults are backward compatible
Given a fresh pi session starts
When sharedState is initialized
Then memoryTokenEstimate is 0 (existing behavior preserved)
And designTree is undefined
And openspec is undefined
And cleave is undefined

### S3: Multiple extensions can read/write concurrently
Given design-tree writes sharedState.designTree
And cleave writes sharedState.cleave
When dashboard reads both properties
Then it sees the latest values from each producer
And no race conditions occur (single-threaded JS)
