+++
id = "13598706-bf83-4dbd-ac10-2f2dd5c8c873"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# effort-tiers — Tasks

## 1. Tier configuration returns correct EffortConfig for each level

- [ ] 1.1 Servitor tier is fully local
- [ ] 1.2 Substantial tier is the daily driver
- [ ] 1.3 Omnissiah tier is all opus
- [ ] 1.4 Tier name resolves to level number
- [ ] 1.5 Unknown tier name returns undefined
- [ ] 1.6 Write tests for Tier configuration returns correct EffortConfig for each level

## 2. Effort state is accessible via shared state

- [ ] 2.1 Shared state populated on session start
- [ ] 2.2 PI_EFFORT env var overrides default
- [ ] 2.3 .pi/config.json effort key sets default
- [ ] 2.4 Env var takes priority over config file
- [ ] 2.5 Write tests for Effort state is accessible via shared state

## 3. /effort command switches tier mid-session

- [ ] 3.1 Switch to named tier
- [ ] 3.2 Show current tier with no args
- [ ] 3.3 Invalid tier name shows error
- [ ] 3.4 Write tests for /effort command switches tier mid-session

## 4. /effort cap locks the ceiling, agent can only downgrade

- [ ] 4.1 Cap prevents agent upgrade
- [ ] 4.2 Cap allows agent downgrade
- [ ] 4.3 Uncap restores full agent control
- [ ] 4.4 Cap state visible in /effort output
- [ ] 4.5 Write tests for /effort cap locks the ceiling, agent can only downgrade

## 5. model-budget respects effort cap on upgrades

- [ ] 5.1 No cap allows any switch
- [ ] 5.2 Cap blocks upgrade past ceiling
- [ ] 5.3 Cap allows lateral and downward switches
- [ ] 5.4 Write tests for model-budget respects effort cap on upgrades

## 6. Cleave reads effort config for dispatch decisions

- [ ] 6.1 Servitor tier forces all children local
- [ ] 6.2 Absolute tier raises the floor to sonnet
- [ ] 6.3 Effort not set falls back to default behavior
- [ ] 6.4 Write tests for Cleave reads effort config for dispatch decisions

## 7. Shared state includes effort field with cap state

- [ ] 7.1 Effort field structure
- [ ] 7.2 Write tests for Shared state includes effort field with cap state
