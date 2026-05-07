+++
id = "68489c1c-4d58-4f44-9514-6786371d5752"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Runtime Binary Identity

### Requirement: Omegon is the canonical executable boundary
The documented and verified operator lifecycle MUST enter through `omegon` as the canonical executable.

#### Scenario: Fresh install uses omegon as the happy path
- **Given** a fresh Omegon install
- **When** the operator follows install and startup instructions
- **Then** they are instructed to launch `omegon`
- **And** no required happy-path step tells them to invoke `pi` directly

### Requirement: Update verification proves Omegon ownership
The `/update` lifecycle MUST verify the active Omegon-owned executable path before handing off to restart.

#### Scenario: Update completes with Omegon-first verification and restart handoff
- **Given** a dev-mode or installed Omegon environment
- **When** `/update` completes successfully
- **Then** the verification summary proves the active `omegon` executable resolves to the Omegon runtime root
- **And** the completion message tells the operator to restart or relaunch `omegon`
- **And** the flow does not require `pi` as the next step

### Requirement: Legacy pi compatibility must not bypass Omegon control
If a legacy `pi` alias remains available, it MUST resolve to the same Omegon-owned entrypoint and runtime checks as `omegon`.

#### Scenario: pi alias re-enters the Omegon boundary
- **Given** an environment where `pi` still exists for compatibility
- **When** the operator invokes `pi --where`
- **Then** the alias resolves through the same Omegon-owned runtime metadata as `omegon --where`
- **And** it does not create a separate lifecycle path outside Omegon control
