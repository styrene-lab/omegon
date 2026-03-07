# Frontmatter — Branch and OpenSpec Fields

## Requirements

### REQ-1: Parse branches and openspec_change from frontmatter

#### Scenario: Frontmatter with branches array

- Given a design doc with frontmatter containing `branches: [feature/auth, fix/auth-rbac]`
- When the doc is parsed
- Then the DesignNode has branches `["feature/auth", "fix/auth-rbac"]`

#### Scenario: Frontmatter with openspec_change

- Given a design doc with frontmatter containing `openspec_change: auth-migration`
- When the doc is parsed
- Then the DesignNode has openspec_change `"auth-migration"`

#### Scenario: Frontmatter without branches defaults to empty array

- Given a design doc with no branches field in frontmatter
- When the doc is parsed
- Then the DesignNode has branches `[]`

### REQ-2: Serialize branches and openspec_change to frontmatter

#### Scenario: Writing branches to frontmatter

- Given a DesignNode with branches `["feature/auth"]`
- When the doc is serialized
- Then the frontmatter contains `branches:` with the branch list

#### Scenario: Empty branches omitted from frontmatter

- Given a DesignNode with branches `[]`
- When the doc is serialized
- Then the frontmatter does not contain a `branches:` key