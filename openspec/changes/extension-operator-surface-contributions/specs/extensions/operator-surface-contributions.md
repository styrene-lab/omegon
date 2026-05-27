# extensions/operator-surface-contributions — Delta Spec

## ADDED Requirements

### Requirement: UI contribution capability defaults off

Extensions SHALL only participate in operator-surface contribution discovery when
they explicitly declare `ui_contributions = true`.

#### Scenario: Legacy extension has no UI contributions
Given an extension capability payload omits `ui_contributions`
When the host decodes capabilities
Then `ui_contributions` is false
And the host does not call `ui/list_contributions`

### Requirement: Runtime contributions stay within manifest envelope

The host SHALL reject runtime UI contributions that exceed the extension's
manifest-declared envelope.

#### Scenario: Runtime command not in manifest is rejected
Given a manifest declares only a `reader` command contribution with id `status`
When `ui/list_contributions` returns a command id `open`
Then the host rejects the `open` contribution
And records a diagnostic explaining that the command exceeds the manifest envelope

### Requirement: Reader can declare delegated document surface

Reader SHALL be able to declare a delegated `document_reader` surface with host
selected placement.

#### Scenario: Reader declares delegated document reader surface
Given Reader declares `ui_contributions = true`
And the manifest includes a surface with `surface_type = document_reader`
And `rendering = delegated`
When the host validates matching runtime contributions
Then the Reader surface is accepted
And preferred placements are preserved in order

### Requirement: Simple extensions can declare host-rendered primitive list surface

Simple extensions SHALL be able to declare a host-rendered `primitive_view` list
surface without owning UI rendering.

#### Scenario: Scratchpad declares host-rendered list surface
Given Scratchpad declares `ui_contributions = true`
And the manifest includes `surface_type = primitive_view`
And `rendering = host`
And `view.primitive = list`
When runtime contributions match the manifest envelope
Then the host accepts the surface
And marks rendering ownership as host

### Requirement: Raw drawing remains unsupported

Extensions SHALL NOT contribute raw terminal drawing, arbitrary ANSI, arbitrary
HTML/JS, or direct keybindings in the MVP.

#### Scenario: Raw drawing contribution is rejected
Given an extension returns a contribution with kind `raw_terminal`
When the host validates runtime contributions
Then the contribution is rejected
And no host UI registry entry is created
