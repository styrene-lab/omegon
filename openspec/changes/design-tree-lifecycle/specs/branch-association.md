# Branch Association — Convention-based Auto-detection

## Requirements

### REQ-1: Branch names containing node IDs are auto-associated

#### Scenario: Fix branch matches implementing node

- Given a design node with id `skill-aware-dispatch` and status `implementing`
- And the node's branches are `["feature/skill-aware-dispatch"]`
- When a branch named `fix/skill-aware-dispatch-rbac` is created
- Then `fix/skill-aware-dispatch-rbac` is appended to the node's branches array

#### Scenario: Longest node ID match wins

- Given two implementing nodes: `auth` and `auth-migration`
- When a branch named `fix/auth-migration-tokens` is created
- Then it is associated with `auth-migration` (longer match wins)
- And it is NOT associated with `auth`

#### Scenario: Non-matching branch is ignored

- Given a design node with id `skill-aware-dispatch` and status `implementing`
- When a branch named `feature/new-unrelated-thing` is created
- Then the node's branches array is unchanged

#### Scenario: Only implementing nodes are candidates

- Given a design node with id `auth` and status `decided`
- When a branch named `fix/auth-bug` is created
- Then no association is made (node is not implementing)

### REQ-2: Segment matching prevents false substring matches

#### Scenario: Segment boundary matching

- Given an implementing node with id `auth`
- When a branch named `feature/authorization-refactor` is created
- Then no association is made (auth is a substring but not a segment of authorization)

#### Scenario: Hyphen-separated segments match correctly

- Given an implementing node with id `skill-aware-dispatch`
- When a branch named `fix/skill-aware-dispatch-rbac` is created
- Then the node ID matches as a segment prefix