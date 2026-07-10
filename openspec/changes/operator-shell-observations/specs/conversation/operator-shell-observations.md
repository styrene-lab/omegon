+++
title = "Operator Shell Observation Delta Spec"
tags = ["openspec","spec","conversation","shell"]
+++

# Operator Shell Observation Delta Spec

# Conversation — Operator Shell Observation Delta Spec

## ADDED Requirements

### Requirement: Operator shell executions are canonical observations

A completed command submitted through the `!` operator surface SHALL be stored in canonical conversation state with operator provenance, execution identity, Bash arguments, working directory, result content, error/exit status, and duration.

#### Scenario: Successful command becomes model-visible evidence
Given the operator submits `!git status`
When the command completes successfully
Then canonical conversation state contains an operator-originated Bash observation
And the next model request includes the command, working directory, exit status, and bounded output
And the observation does not claim the assistant initiated the command

#### Scenario: Failed command remains visible
Given the operator submits a command that exits non-zero
When execution completes
Then canonical conversation state records the non-zero status and output
And the next model request identifies the execution as failed operator-run evidence

### Requirement: Provider-safe model projection

Operator tool observations SHALL project through a provider-safe user-role representation rather than fabricated assistant tool calls or orphaned tool results.

#### Scenario: Provider replay remains structurally valid
Given canonical history contains an operator shell observation
When an Anthropic, OpenAI, or Gemini request is built
Then the observation is represented as attributed user-role context
And no assistant tool-call identifier is fabricated
And no unmatched tool-result block is emitted

### Requirement: Operator observations persist

Operator tool observations SHALL survive session snapshot save and restore while preserving backward compatibility with snapshots that predate the observation type.

#### Scenario: Restored session retains shell evidence
Given a session contains a completed operator shell observation
When the session is saved and restored
Then the command, cwd, status, bounded result, and provenance are retained

### Requirement: Shell results render as terminal output

Bash command source and Bash result output SHALL use separate rendering semantics. Command arguments MAY use Bash syntax highlighting; stdout/stderr SHALL use ANSI-aware terminal rendering with sanitized plaintext fallback.

#### Scenario: ANSI output remains styled after completion
Given an operator command emits ANSI SGR-colored output
When its tool card transitions from running to complete
Then supported foreground, background, and modifier styling remains visible
And unsupported terminal controls are removed
And raw escape bytes are not displayed

#### Scenario: Plain output is not source-highlighted
Given an operator command emits plaintext without ANSI sequences
When the completed result is rendered
Then it uses neutral terminal text styling
And it is not parsed as Bash source

#### Scenario: ANSI parsing failure is safe
Given malformed terminal escape sequences occur in command output
When the output is rendered or projected to model context
Then unsupported controls are removed
And readable plaintext remains available
