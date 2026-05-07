+++
id = "8f2edc1f-a48d-4f22-af0b-b31ba885fdd8"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# harness/slash-command-bridge — Delta Spec

## ADDED Requirements

### Requirement: All OpenSpec commands are agent-callable via execute_slash_command

OpenSpec lifecycle commands (/opsx:propose, /opsx:spec, /opsx:ff, /opsx:status, /opsx:verify, /opsx:archive, /opsx:apply) must be registered with the SlashCommandBridge so the agent can invoke them through the execute_slash_command tool and receive structured result envelopes.

#### Scenario: agent invokes /opsx:status through the bridge
Given the slash-command bridge is initialized
When the agent calls execute_slash_command with command \"opsx:status\"
Then the bridge returns a structured result with ok=true
And the result contains a changes array with name, stage, and task progress for each active change

#### Scenario: agent invokes /opsx:verify through the bridge
Given an OpenSpec change exists with a passing assessment
When the agent calls execute_slash_command with command \"opsx:verify\" and args [\"my-change\"]
Then the bridge returns a structured result with verification substate and next action
And the result is machine-readable without scraping prose

#### Scenario: agent invokes /opsx:archive through the bridge
Given an OpenSpec change is in verifying stage with a passing assessment
When the agent calls execute_slash_command with command \"opsx:archive\" and args [\"my-change\"]
Then the bridge returns a structured result indicating archive success or gate refusal
And the result includes operations performed and any lifecycle transitions

#### Scenario: agent invokes /opsx:propose through the bridge
Given the agent needs to create a new OpenSpec change
When the agent calls execute_slash_command with command \"opsx:propose\" and args [\"new-feature\", \"New Feature Title\"]
Then the bridge returns a structured result with the created change path
And sideEffectClass is \"workspace-write\"

#### Scenario: agent invokes /opsx:ff through the bridge
Given an OpenSpec change exists with specs
When the agent calls execute_slash_command with command \"opsx:ff\" and args [\"my-change\"]
Then the bridge returns a structured result with the generated files list
And sideEffectClass is \"workspace-write\"

### Requirement: Interactive-only commands are registered but not agent-callable

Commands that are inherently interactive (dashboard toggle, visual overlays) should be registered with the bridge with agentCallable: false so the bridge returns a structured refusal rather than an opaque \"not registered\" error.

#### Scenario: agent attempts interactive-only command and gets structured refusal
Given /dashboard is registered with agentCallable: false
When the agent calls execute_slash_command with command \"dashboard\"
Then the bridge returns ok=false with a human-readable explanation that the command is interactive-only
And the response is structured, not an opaque \"not registered\" error

### Requirement: Bridge registration preserves existing interactive UX

Bridging a command must not break the operator-facing slash-command experience. Interactive handlers must render from the same structured result the agent receives.

#### Scenario: interactive /opsx:status renders from structured result
Given the bridge registers /opsx:status with a structuredExecutor
When an operator types /opsx:status interactively
Then the command renders a notification with change status exactly as before
And the notification content is derived from the structuredExecutor result

### Requirement: Bridge metadata declares correct side-effect classes

Each bridged command must declare its side-effect class so the agent and safety layer know what kind of mutation a command performs.

#### Scenario: read-only commands declare read side-effect class
Given /opsx:status and /opsx:verify are registered with the bridge
When the bridge metadata is inspected
Then both commands have sideEffectClass \"read\"
And agentCallable is true

#### Scenario: write commands declare workspace-write side-effect class
Given /opsx:propose, /opsx:ff, and /opsx:archive are registered with the bridge
When the bridge metadata is inspected
Then all three commands have sideEffectClass \"workspace-write\"
And agentCallable is true
