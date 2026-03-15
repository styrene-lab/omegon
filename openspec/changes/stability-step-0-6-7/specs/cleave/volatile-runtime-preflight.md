## ADDED Requirements

### Requirement: Cleave preflight ignores volatile runtime operator profile churn
Cleave dirty-tree preflight MUST classify runtime operator profile churn as volatile so dispatch is not blocked by session-local state.

#### Scenario: operator profile change is volatile
- **GIVEN** `.pi/runtime/operator-profile.json` is modified in the working tree
- **WHEN** cleave runs dirty-tree preflight
- **THEN** the file is classified as volatile
- **AND** it is excluded from checkpoint scope
- **AND** volatile-only dirt does not block child dispatch
