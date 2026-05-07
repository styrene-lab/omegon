+++
id = "1018163d-461d-4dcd-b521-a491fd465179"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# effort-tiers — Design

## Spec-Derived Architecture

### effort

- **Tier configuration returns correct EffortConfig for each level** (added) — 5 scenarios
- **Effort state is accessible via shared state** (added) — 4 scenarios
- **/effort command switches tier mid-session** (added) — 3 scenarios
- **/effort cap locks the ceiling, agent can only downgrade** (added) — 4 scenarios
- **model-budget respects effort cap on upgrades** (added) — 3 scenarios
- **Cleave reads effort config for dispatch decisions** (added) — 3 scenarios
- **Shared state includes effort field with cap state** (added) — 1 scenarios

## File Changes

<!-- Add file changes as you design the implementation -->
