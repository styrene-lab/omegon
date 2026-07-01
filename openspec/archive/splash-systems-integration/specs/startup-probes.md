# Startup Probes — Delta Spec

Retroactive spec (2026-07-01): this change was implemented and verified before spec registration; scenarios below document the shipped behavior for baseline merge.

## ADDED Requirements

### Requirement: Splash checklist reflects real system probes

The splash screen renders nine capability items (cloud, local, hardware, memory, tools, design, secrets, container, mcp) whose states come from real startup probes, not frame-count timers.

#### Scenario: Probe result updates splash item
Given the splash screen is displaying nine pending items
When a ProbeResult for the local-model probe arrives with state Done and summary "ollama: 7 models"
Then the matching item transitions to Done
And the parenthetical summary is rendered next to the label

#### Scenario: Failed probe is visible
Given a probe cannot reach its target
When its ProbeResult arrives with state Failed
Then the matching item renders in the failure style
And splash dismissal is not blocked by the failure

### Requirement: Probes run in parallel within the splash window

All probes run concurrently and complete within 2 seconds so the animation masks probe latency.

#### Scenario: Probes complete under the splash budget
Given the TUI starts and spawns run_probes
When all probes complete
Then each result was delivered through the channel as it finished
And total probe time stays within the 2-second budget

### Requirement: Splash dismissal requires all probes resolved

The splash is ready to dismiss only when all nine items are Done or Failed.

#### Scenario: Pending probe holds splash
Given eight items are resolved and one is Pending
When ready_to_dismiss() is evaluated
Then it returns false

### Requirement: Probe results classify a capability tier

Probe results derive a CapabilityTier (FullCloud, BeefyLocal, FreeCloud, SmallLocal, Offline) stored on the App for routing and tutorial use.

#### Scenario: Cloud credentials present
Given the cloud probe found provider credentials
When classify_tier runs
Then the tier is FullCloud

#### Scenario: No cloud and no local models
Given no cloud credentials and no local model server responded
When classify_tier runs
Then the tier is Offline

### Requirement: Grid layout degrades on narrow terminals

The 3×3 probe grid falls back to fewer columns when terminal width cannot fit three.

#### Scenario: Narrow terminal
Given a terminal too narrow for three grid columns
When the splash renders
Then the grid falls back to two columns or one column
And rendering does not panic