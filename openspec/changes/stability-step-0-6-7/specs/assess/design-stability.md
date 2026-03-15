## ADDED Requirements

### Requirement: Design assessment must produce deterministic results without nested subprocess fragility
`/assess design` MUST complete even when nested subprocess extension loading would conflict.

#### Scenario: design assessment runs in-process
- **GIVEN** a design node exists under `docs/`
- **WHEN** `/assess design <node-id>` executes in bridged mode
- **THEN** structural checks and acceptance-criteria findings are computed in-process
- **AND** the command returns parseable structured findings without requiring nested subprocess JSON output
