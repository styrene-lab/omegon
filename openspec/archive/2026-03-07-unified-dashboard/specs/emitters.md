# State Emitters

Each producer extension (design-tree, openspec, cleave) must emit its state to sharedState and fire a dashboard update event via pi.events.

## Requirements

### R1: Design tree emits dashboard state
design-tree must write sharedState.designTree with current node counts, focused node, and open questions whenever its internal state changes.

### R2: Design tree removes widget ownership
design-tree must stop calling setWidget("design-tree", ...) — the dashboard subsumes this view.

### R3: OpenSpec emits dashboard state
openspec must write sharedState.openspec with change list, stages, and task counts after session start and after any change mutation.

### R4: Cleave emits dashboard state on transitions
cleave must write sharedState.cleave with status, runId, and children array on every state transition (idle, assessing, planning, dispatching, merging, done, failed).

### R5: Cleave dispatcher emits per-child progress
dispatchChildren in dispatcher.ts must update sharedState.cleave.children[n].status as each child starts, completes, or fails.

### R6: All emitters fire dashboard:update event
After writing to sharedState, each emitter must call pi.events.emit("dashboard:update") to trigger re-render.

## Scenarios

### S1: Design tree emits state on node focus
Given a design tree with 4 nodes (2 decided, 1 exploring, 1 seed)
When the user focuses on the exploring node via design_tree_update focus
Then sharedState.designTree.nodeCount equals 4
And sharedState.designTree.decidedCount equals 2
And sharedState.designTree.focusedNode.id equals the focused node's id
And pi.events emitted "dashboard:update"

### S2: Design tree emits on status change
Given a design tree node in exploring status
When design_tree_update sets its status to decided
Then sharedState.designTree.decidedCount increases by 1
And sharedState.designTree.exploringCount decreases by 1

### S3: Design tree widget removed
Given design-tree extension is loaded
When session starts
Then setWidget("design-tree", ...) is never called
And sharedState.designTree is populated instead

### S4: OpenSpec emits changes on session start
Given openspec/changes/ contains 2 change directories
When session starts and openspec scans for changes
Then sharedState.openspec.changes has 2 entries
And each entry has name, stage, tasksDone, tasksTotal

### S5: Cleave emits idle on session start
Given no cleave operation is running
When session starts
Then sharedState.cleave.status equals "idle"

### S6: Cleave emits dispatching with children during run
Given a cleave run with 3 children is started
When dispatch begins
Then sharedState.cleave.status equals "dispatching"
And sharedState.cleave.children has 3 entries with status "pending"

### S7: Cleave child status updates on completion
Given a cleave dispatch is running with 3 children
When child 1 completes successfully
Then sharedState.cleave.children[0].status equals "done"
And sharedState.cleave.children[0].elapsed is a positive number
And pi.events emitted "dashboard:update"

### S8: Cleave emits done on completion
Given all cleave children have completed
When the merge phase finishes
Then sharedState.cleave.status equals "done"
