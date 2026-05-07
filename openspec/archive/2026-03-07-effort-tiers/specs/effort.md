+++
id = "633e0f77-7c47-4fea-a9df-8b12d7900e25"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# effort — Delta Spec

## ADDED Requirements

### Requirement: Tier configuration returns correct EffortConfig for each level

`tierConfig(level)` is a pure function mapping a numeric level (1-7) to an `EffortConfig` object. Each config specifies: driver model, thinking level, extraction model preference, compaction model preference, cleave preferLocal flag, cleave floor tier, and review model.

#### Scenario: Servitor tier is fully local

Given level 1 (Servitor)
When tierConfig is called
Then driver is "local", thinking is "off", extraction is "local", compaction is "local", cleavePreferLocal is true, cleaveFloor is "local", reviewModel is "local"

#### Scenario: Substantial tier is the daily driver

Given level 3 (Substantial)
When tierConfig is called
Then driver is "sonnet", thinking is "low", extraction is "local", compaction is "local", cleavePreferLocal is false, cleaveFloor is "local", reviewModel is "sonnet"

#### Scenario: Omnissiah tier is all opus

Given level 7 (Omnissiah)
When tierConfig is called
Then driver is "opus", thinking is "high", extraction is "opus", compaction is "opus", cleavePreferLocal is false, cleaveFloor is "opus", reviewModel is "opus"

#### Scenario: Tier name resolves to level number

Given the string "Ruthless"
When parseTierName is called
Then the result is level 4

#### Scenario: Unknown tier name returns undefined

Given the string "Legendary"
When parseTierName is called
Then the result is undefined

### Requirement: Effort state is accessible via shared state

The effort extension writes `sharedState.effort` on initialization and on every `/effort` switch. Other extensions read it at their decision points without direct coupling.

#### Scenario: Shared state populated on session start

Given no PI_EFFORT env var and no .pi/config.json effort key
When session_start fires
Then sharedState.effort contains the default tier config (Substantial, level 3)

#### Scenario: PI_EFFORT env var overrides default

Given PI_EFFORT=Servitor
When session_start fires
Then sharedState.effort.level is 1 and sharedState.effort.name is "Servitor"

#### Scenario: .pi/config.json effort key sets default

Given .pi/config.json contains {"effort": "Ruthless"}
And no PI_EFFORT env var
When session_start fires
Then sharedState.effort.level is 4

#### Scenario: Env var takes priority over config file

Given PI_EFFORT=Omnissiah and .pi/config.json contains {"effort": "Servitor"}
When session_start fires
Then sharedState.effort.level is 7

### Requirement: /effort command switches tier mid-session

The `/effort` command changes the active effort tier, updates shared state, switches the driver model, and notifies the operator.

#### Scenario: Switch to named tier

Given effort is currently Substantial
When the operator runs `/effort Omnissiah`
Then sharedState.effort.level becomes 7
And the driver model is switched to opus
And the thinking level is set to high
And a notification confirms the switch

#### Scenario: Show current tier with no args

Given effort is Ruthless
When the operator runs `/effort`
Then the current tier name, level, and key settings are displayed

#### Scenario: Invalid tier name shows error

Given any current tier
When the operator runs `/effort Legendary`
Then an error notification lists valid tier names

### Requirement: /effort cap locks the ceiling, agent can only downgrade

`/effort cap` locks the current tier as the maximum. `set_model_tier` can downgrade but not upgrade past the cap. `/effort uncap` removes the lock.

#### Scenario: Cap prevents agent upgrade

Given effort is capped at Ruthless (level 4, driver=sonnet)
When the agent calls set_model_tier with tier "opus"
Then the request is rejected
And the tool returns a message explaining the cap

#### Scenario: Cap allows agent downgrade

Given effort is capped at Ruthless (level 4, driver=sonnet)
When the agent calls set_model_tier with tier "haiku"
Then the model switches to haiku

#### Scenario: Uncap restores full agent control

Given effort was capped at Substantial
When the operator runs `/effort uncap`
Then the cap is removed
And the agent can upgrade freely

#### Scenario: Cap state visible in /effort output

Given effort is capped at Ruthless
When the operator runs `/effort`
Then the output shows "CAPPED" indicator

### Requirement: model-budget respects effort cap on upgrades

model-budget's `set_model_tier` tool checks `sharedState.effort` before switching. If a cap is active, upgrades past the cap tier's driver model are rejected.

#### Scenario: No cap allows any switch

Given no effort cap is active
When set_model_tier is called with "opus"
Then the switch succeeds

#### Scenario: Cap blocks upgrade past ceiling

Given effort cap is at Substantial (driver=sonnet)
When set_model_tier is called with "opus"
Then the switch is blocked and returns an explanation

#### Scenario: Cap allows lateral and downward switches

Given effort cap is at Lethal (driver=sonnet/opus)
When set_model_tier is called with "sonnet"
Then the switch succeeds

### Requirement: Cleave reads effort config for dispatch decisions

The cleave dispatcher reads `sharedState.effort` to determine `preferLocal` and the floor tier for child classification.

#### Scenario: Servitor tier forces all children local

Given sharedState.effort.level is 1
When cleave dispatches children
Then all children have executeModel "local" regardless of scope

#### Scenario: Absolute tier raises the floor to sonnet

Given sharedState.effort.level is 6
When cleave classifies a 2-file child
Then the child executeModel is at least "sonnet" (floor overrides scope-based "local")

#### Scenario: Effort not set falls back to default behavior

Given sharedState.effort is undefined
When cleave dispatches children
Then scope-based autoclassification runs normally with no floor override

### Requirement: Shared state includes effort field with cap state

The SharedState interface includes an optional `effort` field containing the current EffortConfig plus a `capped` boolean and `capLevel` number.

#### Scenario: Effort field structure

Given effort is set to Ruthless and capped
When sharedState.effort is read
Then it contains level=4, name="Ruthless", capped=true, capLevel=4, and all config fields
