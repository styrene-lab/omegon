# Archive Gate — OpenSpec Archive Triggers Implemented

## Requirements

### REQ-1: OpenSpec archive sets node to implemented

#### Scenario: Archive transitions implementing node to implemented

- Given a design node with status `implementing` and openspec_change `auth-migration`
- When the OpenSpec change `auth-migration` is archived
- Then the design node status becomes `implemented`

#### Scenario: Archive with no matching design node is a no-op

- Given no design node has openspec_change `orphan-change`
- When the OpenSpec change `orphan-change` is archived
- Then no error occurs and no node status changes

### REQ-2: Only implementing nodes are transitioned

#### Scenario: Archive does not affect decided nodes

- Given a design node with status `decided` and openspec_change `old-change`
- When the OpenSpec change `old-change` is archived
- Then the node status remains `decided`