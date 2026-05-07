+++
id = "c6c4e99e-f2c5-4039-a841-c3773125f6a6"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Runtime Subprocess Entrypoint

### Requirement: Internal subprocesses re-enter the Omegon-owned executable boundary
Internal recursive subprocess launches MUST resolve the canonical Omegon-owned executable contract explicitly instead of relying on PATH lookup of the legacy `pi` alias.

#### Scenario: Cleave child dispatch uses canonical Omegon executable resolution
- **Given** a cleave child task is dispatched from inside an Omegon session
- **When** the dispatcher launches the child subprocess
- **Then** it uses the shared Omegon executable resolver rather than spawning bare `pi` by name
- **And** the child still receives the same non-interactive flags and environment expected for cleave execution

#### Scenario: Bridged assessment helpers use canonical Omegon executable resolution
- **Given** `/assess spec` or `/assess design` runs through the bridged in-band subprocess path
- **When** the helper process is launched
- **Then** it uses the shared Omegon executable resolver rather than spawning bare `pi` by name
- **And** the existing structured JSON assessment flow remains intact

#### Scenario: Project-memory subprocess fallback uses canonical Omegon executable resolution
- **Given** project-memory falls back to a subprocess-based extraction path
- **When** that helper subprocess is launched
- **Then** it uses the shared Omegon executable resolver rather than spawning bare `pi` by name
- **And** existing timeout, detachment, and stderr/stdout handling semantics are preserved

#### Scenario: Side-by-side installations cannot redirect internal subprocesses via PATH
- **Given** a machine where Omegon and another `pi` executable could both exist on PATH
- **When** Omegon launches an internal helper subprocess
- **Then** the subprocess re-enters the same Omegon-owned runtime boundary as the current session
- **And** correctness does not depend on PATH selecting the compatibility alias
