# Dashboard — Implementing/Implemented Rendering

## Requirements

### REQ-1: Compact mode shows implementing count

#### Scenario: Compact footer with implementing nodes

- Given 1 decided node and 2 implementing nodes out of 5 total
- When the compact footer renders
- Then it displays `◈ D:1 I:2 /5`

### REQ-2: Raised mode shows implementing details

#### Scenario: Raised footer with implementing node and branch

- Given an implementing node `skill-aware-dispatch` with branches `["feature/skill-aware-dispatch"]`
- When the raised footer renders
- Then it shows implementing count in the status line
- And shows `⚙ skill-aware-dispatch → feature/skill-aware-dispatch` with accent color

### REQ-3: Emitter includes new counts

#### Scenario: Design tree emitter populates implementingCount

- Given 2 nodes with status implementing and 1 with status implemented
- When emitDesignTreeState runs
- Then sharedState.designTree.implementingCount is 2
- And sharedState.designTree.implementedCount is 1