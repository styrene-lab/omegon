+++
id = "c5d449dc-b253-4c6a-b425-13dbcd5feb2a"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# OpenSpec Skill Task Generation Spec

## MODIFIED Requirements

### Requirement: Scenario-first grouping guidance in skill

The openspec skill must instruct the LLM to group tasks by spec domain rather than file layer.

#### Scenario: Skill includes grouping instructions
Given the skills/openspec/SKILL.md file
When the task generation section is read
Then it contains instructions to group tasks by spec domain end-to-end
And it instructs not to split a spec domain's implementation across groups by architectural layer
And it instructs to include `<!-- specs: domain/name -->` annotations on each group header

#### Scenario: Example shows scenario-first grouping
Given the skills/openspec/SKILL.md file
When the task generation examples are read
Then at least one example demonstrates grouping by spec domain (e.g., "RBAC Enforcement" group that spans model + service files)
And the example includes the `<!-- specs: ... -->` annotation
