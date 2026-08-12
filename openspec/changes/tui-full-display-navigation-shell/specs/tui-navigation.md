# TUI navigation — Delta Spec

## ADDED Requirements

### Requirement: Exclusive interactive display ownership

When an interactive menu, inspector, picker, or blocking prompt is active in the inline TUI, it must own the complete live display and must not share geometry with conversation composition or permanent fixtures.

#### Scenario: Settings replaces the conversation frame
Given the inline TUI is displaying conversation, editor, and Workbench fixtures
When the operator opens `/settings`
Then the Settings navigation surface occupies the complete live frame
And conversation, editor, dashboard, Workbench, and status fixtures are not rendered behind or beside it
And completed conversation remains available in native terminal scrollback

#### Scenario: Constrained terminal remains usable
Given an interactive menu is active in a terminal with constrained height
When the selected row moves beyond the initially visible rows
Then the selected row remains visible
And hidden rows are represented by explicit overflow indicators
And menu chrome does not reserve centered-modal margins

### Requirement: Canonical settings hierarchy

Settings must be the canonical hierarchy for operator-configurable runtime and capability areas. Task-oriented slash commands must deep-link to destination identities in that hierarchy rather than opening parallel menu state.

#### Scenario: Extensions alias deep-links
Given no interactive surface is active
When the operator invokes `/extensions`, `/extension`, or `/settings extensions`
Then each command opens the same Settings > Extensions destination identity
And the destination uses the extension inventory projection
And no command is recursively fed through the editor

#### Scenario: Bare settings is deterministic
Given the operator previously visited a child settings destination
When the operator invokes bare `/settings`
Then the Settings root opens
And the prior child destination is not silently restored

### Requirement: Hierarchical return semantics

The navigation shell must retain parent destination state while the operator visits child destinations or value selectors.

#### Scenario: Back returns to parent state
Given the operator selected an extension row from a filtered Extensions destination
When the operator opens its detail page and then activates Back
Then the Extensions destination is restored
And its filter and selected row are preserved

#### Scenario: Selector cancellation returns to settings
Given the operator opens a model selector from Settings > Model & inference
When the operator cancels the selector
Then Settings > Model & inference is restored
And the navigation root remains active

#### Scenario: Escape unwinds nearest scope
Given a child settings destination has an active search or inline value editor
When the operator presses Escape
Then the transient editor closes first
And the child destination remains open
When the operator presses Escape again
Then the parent destination is restored
When the operator presses Escape at the root
Then the navigation shell closes and conversation mode resumes

### Requirement: Viewport restoration

Entering and leaving an exclusive interactive surface must preserve native scrollback and restore compact conversation geometry.

#### Scenario: Closing settings restores conversation
Given completed exchanges exist in terminal scrollback and Settings is active
When the operator closes the Settings root
Then the compact inline conversation viewport is restored
And completed exchanges are not republished into scrollback
And permanent fixtures use the restored compact geometry

### Requirement: Non-settings interactive surfaces

Inspectors and blocking prompts must use exclusive display ownership without being represented as settings destinations.

#### Scenario: Process detail is an inspector
Given a retained process session is available
When the operator opens its detail viewer
Then a full-display inspector is rendered
And it is not placed under the Settings breadcrumb
And it does not use centered modal geometry

#### Scenario: Legacy extension modal compatibility
Given an installed extension emits a legacy modal payload
When the TUI presents that payload
Then it is adapted to an exclusive full-display compatibility page
And conversation content is not rendered concurrently
