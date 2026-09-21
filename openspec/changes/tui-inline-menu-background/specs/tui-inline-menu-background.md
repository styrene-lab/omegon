# Inline navigation background - Delta Spec

## ADDED Requirements

### Requirement: Borrowed navigation space does not activate the fullscreen workspace

When an inline session borrows the alternate screen for navigation, the application
renders the owning rich surface on a clean background. It does not render the
conversation, composer, workspace panels, or fullscreen footer beneath that surface.
Shared widgets and input precedence remain authoritative.

#### Scenario: Settings over a resumed inline session
Given an inline session with existing historical conversation excluded from automatic publication
When the operator opens settings or its model selector
Then the requested menu appears on a clean background
And historical conversation, composer, and fullscreen workspace chrome remain absent
And the canonical conversation and publication boundary remain unchanged

#### Scenario: Covered root retains decision ownership
Given an inline session with a mounted rich navigation root and a pending decision
When the decision is rendered
Then the decision owns visible input on the borrowed clean screen
And covered roots and absent workspace surfaces cannot receive decision input

#### Scenario: Explicit fullscreen retains the workspace
Given the operator has explicitly selected the fullscreen terminal base
When the operator opens settings
Then the shared settings widget overlays the normal fullscreen workspace
And historical conversation remains available in that workspace

#### Scenario: Menu settings refresh the inline composer
Given an inline session with a thinking selector open on the borrowed screen
When the operator selects a different thinking level
Then the inline composer reflects the accepted setting without another inference request
And model and context labels continue to reflect shared settings

### Requirement: Dismissing inline navigation preserves the primary surface

Borrowed navigation uses the existing balanced alternate-buffer lifecycle and does
not consume or replay prior native output. It preserves an unsent editor draft.

#### Scenario: Return from settings after resume
Given native primary-buffer text and a resumed inline conversation
When the operator closes settings after inspecting a selector
Then the application returns to the prior inline surface
And native text remains intact without replaying resumed history
And the operator can type and submit the next prompt

#### Scenario: Draft and resize round trip
Given an inline unsent draft with a rich navigation root open
When the terminal is resized and the root is dismissed
Then the inline composer retains the same draft
And stale workspace hit areas are absent during borrowed navigation
