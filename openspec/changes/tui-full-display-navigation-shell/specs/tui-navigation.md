# TUI navigation — Delta Spec

> Historical branch requirements. Current accepted terminal ownership follows
> `../../tui-dual-presentation/`; these deltas have not been merged into its baseline.

## ADDED Requirements

### Requirement: Exclusive interactive display ownership

When a navigation surface, inspector, picker, or blocking prompt is active in the inline TUI, one exclusive-surface owner must receive the complete Ratatui frame and all operator input. It must not share geometry or input dispatch with conversation composition or permanent fixtures. Passive notifications must be queued until conversation mode resumes; a blocking prompt must transition through the exclusive-surface owner with an explicit return target.

#### Scenario: Settings replaces the conversation frame
Given the inline TUI is displaying conversation, editor, and Workbench fixtures
When the operator opens `/settings`
Then the Settings navigation surface occupies the complete live frame
And conversation, editor, dashboard, Workbench, and status fixtures are not rendered behind or beside it
And completed conversation remains available in native terminal scrollback
And a passive toast arriving while Settings is active is queued rather than rendered over Settings

#### Scenario: Input cannot fall through to conversation
Given Settings owns the exclusive surface and the composer contains a draft
When the operator types navigation, search, paste, or mouse input
Then the event is consumed or rejected by the active Settings interaction scope
And the composer draft and conversation selection are unchanged

#### Scenario: Blocking prompt resumes its return target
Given Settings > Extensions is active with a filter and selected row
And a blocking responder-backed prompt becomes pending
When the prompt is admitted and the operator resolves it
Then the prompt exclusively owns input while displayed
And Settings > Extensions resumes with its filter and selected row intact

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

#### Scenario: Settings aliases share canonical identities
Given no interactive surface is active
When the operator invokes one of `/skills`, `/model`, `/auth`, `/settings skills`, `/settings model`, or `/settings auth`
Then each task-oriented alias opens the same destination identity as its `/settings <section>` form
And `/config <section>` resolves identically to `/settings <section>`

#### Scenario: Invalid settings destination is rejected
Given Settings or conversation mode is active
When the operator invokes `/settings` with an unknown or unavailable section
Then no navigation state is discarded
And the operator receives a diagnostic listing or suggesting supported destinations

#### Scenario: Deep link replaces the current settings path
Given Settings > Extensions > extension detail is active
When the operator invokes `/settings skills`
Then the navigation session opens Settings > Skills through the canonical router
And no recursive editor command is executed

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

#### Scenario: Selector confirmation returns to settings
Given the operator opens a model selector from Settings > Model & inference
When the operator confirms an available model
Then the selected model is applied
And Settings > Model & inference is restored with live values refreshed
And the navigation root remains active

#### Scenario: Search Escape closes only search
Given a child settings destination has an active search editor
When the operator presses Escape
Then the search editor closes
And the child destination remains open

#### Scenario: Confirmation Escape cancels only confirmation
Given a settings action is waiting for second-activation confirmation
When the operator presses Escape
Then the confirmation is cancelled
And the current destination remains open

#### Scenario: Child Escape returns to parent
Given a child destination or picker is active under a settings parent
When the operator presses Escape
Then the exact parent destination is restored

#### Scenario: Root Escape restores conversation
Given the Settings root is active with no transient interaction
When the operator presses Escape
Then the navigation shell closes
And conversation mode resumes

#### Scenario: Live refresh reconciles restored selection by identity
Given a parent destination retains a filter and selected row identity while a child is active
And the backing inventory reorders or removes rows
When the operator returns to the parent
Then the filter is retained
And the prior row identity is selected if it remains visible
And otherwise selection clamps to the first available visible row without retargeting an action by stale index

### Requirement: Viewport restoration

The native-transcript TUI must be initialized once with a terminal-height inline viewport. Conversation mode must compose compact fixtures inside that frame, while exclusive surfaces consume the complete frame. Surface transitions must not recreate the terminal, switch alternate-screen state, publish transcript content, or impair later physical terminal resize handling.

#### Scenario: Terminal-height inline viewport preserves scrollback
Given native-transcript mode has completed exchanges in terminal scrollback
When the terminal-height inline viewport renders compact conversation geometry and the operator resizes and scrolls the physical terminal
Then completed exchanges remain reachable in native scrollback
And the live frame remains bounded to the current physical terminal

#### Scenario: Closing settings restores conversation
Given completed exchanges exist in terminal scrollback and Settings is active
When the operator closes the Settings root
Then the compact inline conversation viewport is restored
And completed exchanges are not republished into scrollback
And permanent fixtures use the restored compact geometry
And the Ratatui terminal instance and viewport mode were not replaced during the transition

### Requirement: Settings actions reflect runtime support

A Settings row must not advertise an enabled editor or primary action unless the runtime implements it.

#### Scenario: Unsupported settings editor is unavailable
Given a projected Settings row has no registered runtime editor or destination
When Settings renders the row
Then the row is visibly unavailable with a diagnostic reason
And activating Enter does not emit a mutation command

#### Scenario: Supported settings editor returns refreshed state
Given a projected Settings row has a registered editor
When the operator completes its action
Then the runtime value is updated
And the parent destination resumes with a freshly rebuilt projection

### Requirement: Non-settings interactive surfaces

Inspectors and blocking prompts must use exclusive display ownership without being represented as settings destinations.

#### Scenario: Process detail is an inspector
Given a retained process session is available
When the operator opens its detail viewer
Then a full-display inspector is rendered
And it is not placed under the Settings breadcrumb
And it does not use centered modal geometry

#### Scenario: Legacy extension modal compatibility
Given an installed extension emits a legacy modal payload while conversation mode is active
When the TUI presents that payload
Then it is adapted to an exclusive full-display compatibility page
And conversation content is not rendered concurrently

#### Scenario: Legacy extension event does not replace active navigation
Given Settings is active and an installed extension emits a legacy modal or action-required payload
When the payload becomes pending
Then the active navigation session remains intact
And the payload is queued or admitted as an exclusive prompt with an explicit return target
And a second payload does not silently replace the first

#### Scenario: Document viewer is an inspector
Given copyable markdown or code is available
When the operator opens its document viewer
Then a full-display inspector owns the frame and input
And closing it restores its explicit return target
