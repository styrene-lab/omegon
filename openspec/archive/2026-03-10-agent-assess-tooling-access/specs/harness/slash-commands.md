+++
id = "6f379872-875e-4de9-83ca-0e4b64343205"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

## ADDED Requirements

### Requirement: Allowlisted slash commands are invokable by the harness
The agent harness MUST be able to invoke slash commands through a general bridge only when those commands explicitly opt in as agent-callable.

#### Scenario: harness invokes an allowlisted command
- **Given** a slash command is registered with agent-callable metadata and a structured result contract
- **When** the harness requests execution of that command through the bridge
- **Then** the bridge executes the shared command handler
- **And** returns a structured result envelope instead of only terminal text

#### Scenario: harness refuses a command that is not allowlisted
- **Given** a slash command exists but is not marked agent-callable
- **When** the harness requests that command through the bridge
- **Then** execution is refused
- **And** the result explains that the command is not approved for agent invocation

### Requirement: Bridged commands share one implementation path
Interactive slash-command UX and harness execution MUST run through the same underlying command logic to prevent behavior drift.

#### Scenario: interactive and bridged execution share the same handler
- **Given** a bridged slash command has both interactive and harness entrypoints
- **When** the command is executed from the terminal or through the harness bridge
- **Then** both entrypoints call the same structured executor
- **And** the interactive path only adds human-oriented rendering around the shared result

### Requirement: Bridged commands return structured machine-readable results
The bridge MUST return normalized results that agents can consume without scraping human-oriented output.

#### Scenario: bridge returns normalized envelope
- **Given** a bridged command finishes execution
- **When** the bridge returns the result to the harness
- **Then** the result includes command identity, success state, summary, human-readable text, command-specific structured data, observed effects, and suggested next steps

#### Scenario: assessment command returns lifecycle reconciliation signals
- **Given** a bridged assessment command detects warnings, failures, or reopened work
- **When** the result is returned to the harness
- **Then** the structured data includes severity summaries and follow-up signals relevant to OpenSpec or design-tree reconciliation
- **And** the agent does not need to infer those states by parsing prose alone

### Requirement: Safety metadata governs side effects
The bridge MUST enforce explicit safety metadata for bridged commands and surface confirmation requirements instead of silently executing risky actions.

#### Scenario: operator confirmation is required for a risky command
- **Given** a bridged command is marked as requiring operator confirmation for its side effects
- **When** the harness requests execution without confirmation
- **Then** the bridge does not execute the command
- **And** returns a structured response indicating confirmation is required

#### Scenario: side-effect classification is available to the harness
- **Given** a bridged command is agent-callable
- **When** the harness inspects or executes it
- **Then** the bridge exposes its side-effect classification such as read, workspace-write, git-write, or external-side-effect

### Requirement: V1 prioritizes lifecycle-critical assessment commands
The first commands onboarded to the bridge MUST cover the current lifecycle gap around assessment-driven workflows.

#### Scenario: assess spec is bridge-enabled in v1
- **Given** the bridge is enabled in v1
- **When** the agent needs to validate an OpenSpec change before archive
- **Then** `/assess spec` is invokable through the bridge
- **And** its structured result is sufficient for the agent to determine whether work passed, reopened, or needs reconciliation

#### Scenario: assess diff and assess cleave are bridge-enabled in v1
- **Given** the bridge is enabled in v1
- **When** the agent needs diff review or cleave review results
- **Then** `/assess diff` and `/assess cleave` are invokable through the bridge
- **And** they return structured review outcomes using the shared result contract
