# Types — Design Tree Lifecycle Extension

## Requirements

### REQ-1: NodeStatus includes implementing and implemented

The `NodeStatus` type must include `implementing` and `implemented` as valid values, with corresponding entries in `STATUS_ICONS`, `STATUS_COLORS`, and `VALID_STATUSES`.

#### Scenario: implementing status has correct icon and color

- Given NodeStatus type is defined in types.ts
- When a node has status `implementing`
- Then STATUS_ICONS maps it to `⚙`
- And STATUS_COLORS maps it to `accent`

#### Scenario: implemented status has correct icon and color

- Given NodeStatus type is defined in types.ts
- When a node has status `implemented`
- Then STATUS_ICONS maps it to `✓`
- And STATUS_COLORS maps it to `success`

#### Scenario: VALID_STATUSES includes all seven statuses

- Given VALID_STATUSES array is defined
- Then it contains exactly: seed, exploring, decided, implementing, implemented, blocked, deferred

### REQ-2: DesignNode includes branches and openspec_change

The `DesignNode` interface must include `branches: string[]` and `openspec_change?: string` fields for traceability.

#### Scenario: DesignNode with branch history

- Given a DesignNode with status implementing
- When branches field is populated
- Then it is an array of branch name strings
- And openspec_change is an optional string field

### REQ-3: Dashboard types include implementingCount and implementedCount

#### Scenario: DesignTreeDashboardState has implementation counters

- Given DesignTreeDashboardState interface
- Then it includes implementingCount: number
- And it includes implementedCount: number