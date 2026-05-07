+++
id = "602d5bcc-9629-412d-8f33-4f0bb5040cb3"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Spec

### Requirement: Design assessment must produce deterministic results without nested subprocess fragility
`/assess design` MUST complete even when nested subprocess extension loading would conflict.

#### Scenario: design assessment runs in-process
- **GIVEN** a design node exists under `docs/`
- **WHEN** `/assess design <node-id>` executes in bridged mode
- **THEN** structural checks and acceptance-criteria findings are computed in-process
- **AND** the command returns parseable structured findings without requiring nested subprocess JSON output
