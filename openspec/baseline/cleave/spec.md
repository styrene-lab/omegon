+++
id = "93ee8854-f8e7-402e-a4e5-0b64983ade66"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Cleave Task Generation Spec

### Requirement: Spec-domain annotations in tasks.md

Task group headers may include an HTML comment declaring which spec domains the group owns.

#### Scenario: Parse spec-domain annotation from group header
Given a tasks.md with a group header `## 2. RBAC Enforcement` followed by `<!-- specs: relay/rbac -->`
When parseTasksFile is called
Then the resulting TaskGroup has a `specDomains` field containing `["relay/rbac"]`

#### Scenario: Parse multiple spec domains from annotation
Given a tasks.md group header followed by `<!-- specs: relay/rbac, relay/session -->`
When parseTasksFile is called
Then the resulting TaskGroup has `specDomains` containing `["relay/rbac", "relay/session"]`

#### Scenario: Group with no annotation has empty specDomains
Given a tasks.md group header with no `<!-- specs: ... -->` comment
When parseTasksFile is called
Then the resulting TaskGroup has `specDomains` as an empty array

### Requirement: Orphan scenario detection

After matching spec scenarios to children, any scenario that matches zero children must be detected.

#### Scenario: All scenarios matched
Given a change with 3 spec scenarios and 2 task groups with annotations covering all 3 scenario domains
When buildDesignSection is called for each child
Then every scenario appears in at least one child's acceptance criteria
And no orphan markers are emitted

#### Scenario: Orphan scenario auto-injected
Given a change with a spec scenario in domain `relay/rbac` and no task group has a `<!-- specs: relay/rbac -->` annotation
When orphan detection runs after scenario matching
Then the orphaned scenario is injected into the child whose file scope best matches the scenario's enforcement point
And the injected scenario is prefixed with `⚠️ CROSS-CUTTING`

#### Scenario: Orphan injected by scope match
Given an orphan scenario whose When clause references `create_session`
And child A's scope includes `relay_service.py` (where create_session is defined)
And child B's scope includes `rbac.py`
When selecting the injection target
Then child A is selected (scope contains the enforcement file)

#### Scenario: Orphan injected by word overlap fallback
Given an orphan scenario about "rate limiting" with no file scope match to any child
When selecting the injection target using word overlap fallback
Then the child with the most word overlap in its description is selected

### Requirement: Annotation-first scenario matching

Scenario-to-child matching should use spec-domain annotations as the primary mechanism, with fallbacks.

#### Scenario: Annotation match takes precedence
Given a child with `<!-- specs: relay/rbac -->` annotation
And the change has scenarios in domain `relay/rbac`
When buildDesignSection matches scenarios to children
Then all `relay/rbac` scenarios are assigned to this child
And the word-overlap heuristic is not consulted for these scenarios

#### Scenario: Fallback to scope-based matching
Given a scenario in domain `relay/config` with no annotation match
And child A's scope includes files under `config/`
When buildDesignSection matches scenarios
Then the scenario falls back to scope-based matching and is assigned to child A

#### Scenario: Fallback to word-overlap matching
Given a scenario with no annotation match and no scope match
When buildDesignSection matches scenarios
Then the scenario falls back to word-overlap matching (existing behavior)

## MODIFIED Requirements

### Requirement: TaskGroup type includes specDomains

#### Scenario: TaskGroup interface extended
Given the TaskGroup type in openspec.ts
When a tasks.md is parsed
Then each TaskGroup object includes a `specDomains: string[]` field populated from the `<!-- specs: ... -->` annotation (empty array if absent)
