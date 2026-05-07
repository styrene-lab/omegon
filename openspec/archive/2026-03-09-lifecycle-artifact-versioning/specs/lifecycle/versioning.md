+++
id = "be310c15-a370-4e0a-b0a3-bee4665f03a5"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# lifecycle/versioning — Delta Spec

## ADDED Requirements

### Requirement: Durable lifecycle artifacts are version controlled

Design-tree documents and OpenSpec change artifacts that describe project intent, decisions, plans, or verification state MUST be committed to git so they can serve as durable project documentation.

#### Scenario: Design-tree and OpenSpec files are treated as durable artifacts
Given a repository contains design-tree documents under `docs/` and OpenSpec change files under `openspec/`
When an operator creates or updates those files as part of implementation work
Then repository policy treats them as version-controlled artifacts rather than disposable scratch files
And the contributor guidance explains that these files are part of the durable project record

### Requirement: Transient cleave runtime artifacts remain optional

Cleave runtime workspaces, worktrees, and other transient execution artifacts MAY remain outside version control when they are machine-local or reconstructed from durable lifecycle artifacts.

#### Scenario: Transient cleave workspace outputs are excluded from the durability requirement
Given cleave creates temporary workspaces or worktrees outside the repository for execution
When repository lifecycle artifact policy is enforced
Then those transient runtime artifacts are not required to be committed
And the policy distinguishes them from repo-local planning artifacts such as OpenSpec tasks and design notes

### Requirement: Repository checks detect untracked durable lifecycle artifacts

The repository MUST provide an automated check that fails or warns loudly when durable lifecycle artifacts are left untracked, with actionable guidance for how to resolve the issue.

#### Scenario: Untracked OpenSpec or design-tree files are surfaced by automated checks
Given a contributor has created or modified durable lifecycle artifacts under `docs/` or `openspec/`
And one or more of those files are still untracked in git
When the contributor runs the standard repository validation command
Then the validation reports the untracked lifecycle artifact paths explicitly
And the message tells the contributor to `git add` the files or intentionally relocate transient artifacts outside the durable lifecycle paths
