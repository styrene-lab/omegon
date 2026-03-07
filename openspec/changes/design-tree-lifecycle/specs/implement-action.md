# Implement Action — Auto-transition and Branch Creation

## Requirements

### REQ-1: Implement action transitions to implementing and creates branch

#### Scenario: Implement scaffolds OpenSpec and transitions status

- Given a design node with status `decided` and id `auth-strategy`
- When the `implement` action is invoked
- Then the node status becomes `implementing`
- And the node's openspec_change field is set to `auth-strategy`
- And the node's branches field contains `feature/auth-strategy`
- And an OpenSpec change directory is scaffolded

#### Scenario: Implement creates git branch with feature/ prefix

- Given a design node with id `skill-aware-dispatch`
- When the `implement` action is invoked
- Then a git branch named `feature/skill-aware-dispatch` is created
- And the working tree is switched to that branch

#### Scenario: Implement with explicit branch override

- Given a design node with id `auth-strategy` and frontmatter `branch: refactor/auth-overhaul`
- When the `implement` action is invoked
- Then a git branch named `refactor/auth-overhaul` is created
- And branches field contains `refactor/auth-overhaul`

### REQ-2: Implement still requires decided status

#### Scenario: Implement rejects non-decided nodes

- Given a design node with status `exploring`
- When the `implement` action is invoked
- Then it returns an error indicating the node must be decided first