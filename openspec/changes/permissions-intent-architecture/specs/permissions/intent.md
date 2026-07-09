# Permissions Intent — Delta Spec

## ADDED Requirements

### Requirement: Filesystem access is represented as structured intent

Filesystem permission evaluation MUST receive operation, target, actor/source provenance, and extraction confidence instead of only a raw path string wherever the caller can provide that data.

#### Scenario: Exact tool path carries provenance
Given a tool requests access to a path through a typed path argument
When the permission system evaluates the request
Then the request includes the tool name and field name as provenance
And the confidence is `Exact`.

#### Scenario: Shell-derived path carries source excerpt
Given a bash command contains a filesystem write target
When shell preflight extracts an intent
Then the intent includes the operation
And the intent includes a source excerpt or command/argument provenance
And the confidence reflects whether extraction was parsed or heuristic.

### Requirement: Path resolution emits suspicious-path diagnostics

Path resolution MUST classify suspicious absolute paths without rewriting them.

#### Scenario: Root-dot path is diagnosed
Given a command requests `/.omegon/runtime`
When the permission system resolves the target
Then the target remains host-absolute
And the diagnostics include that `.omegon/runtime` may have been intended as a workspace-relative path.

#### Scenario: Short root path is diagnosed
Given a shell scanner extracts `/Ig` from model-authored command text
When the permission system resolves the target
Then diagnostics mark it as a suspicious short root path.
