## ADDED Requirements

### Requirement: Memory injection stays within a tighter routine-turn budget
Omegon MUST reduce routine project-memory prompt injection so normal turns do not prepend an oversized memory block by default.

#### Scenario: routine turn uses a reduced default budget
- **GIVEN** project memory is available for the active mind
- **WHEN** Omegon prepares a normal per-turn memory injection
- **THEN** the default project-memory selection budget is materially lower than the previous 15%-of-context policy
- **AND** high-priority working-memory facts still take precedence over lower-priority filler content

### Requirement: Low-value additive memory is conditional
Omegon MUST avoid appending episodic, global, or structural filler memory unless those additions are justified by the current turn and remaining budget.

#### Scenario: filler content is skipped on low-signal turns
- **GIVEN** a short or low-signal user turn with no strong cross-project need
- **WHEN** project-memory builds the injected context block
- **THEN** episodic memory and cross-project global facts are omitted by default
- **AND** structural filler facts are only added when enough budget remains after higher-priority content

### Requirement: Memory telemetry remains operator-auditable
Omegon MUST continue surfacing enough telemetry to validate the effect of memory-budget changes.

#### Scenario: injection metrics still expose payload size
- **GIVEN** a turn that injects project memory
- **WHEN** Omegon records the last memory injection snapshot
- **THEN** the snapshot includes payload size and estimated token cost
- **AND** operators can compare the injected payload against baseline prompt usage
