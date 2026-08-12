# TUI presentation — Delta Spec

## ADDED Requirements

### Requirement: Canonical presentation controls

The renderer-neutral Settings projection SHALL expose presentation level, tool density, startup splash policy, named theme, requested glyph profile, effective glyph profile, and Nerd Font remediation from the UI tab.

#### Scenario: Existing presentation controls are discoverable
Given the operator opens the canonical Settings UI tab
When the settings projection is built
Then presentation level, tool density, and startup splash policy are editable choices
And their active choices match persisted runtime settings

#### Scenario: Theme can be changed live and persisted
Given a bundled theme is available
When the operator selects it from the UI settings destination
Then the active TUI theme changes without restarting
And the selected theme is persisted through the profile settings path

### Requirement: Capability-aware glyph preference

The system SHALL distinguish the operator's requested glyph preference from detected terminal capability and effective glyph profile.

#### Scenario: Automatic glyph selection follows detection
Given glyph preference is auto
When terminal capability is resolved
Then the effective glyph profile equals the detected profile
And Settings reports both the request and effective result

#### Scenario: Explicit safe profile is honored
Given the operator selects Unicode or ASCII
When glyph preference is resolved
Then the effective profile matches the explicit selection
And the choice persists through the profile settings path

#### Scenario: Unsupported Nerd Font request is visible
Given the operator requests Nerd Font glyphs
And detection does not establish Nerd Font support
When Settings is projected
Then the request remains persisted
And the row has warning status
And a remediation action exposes detection evidence and the Nerd Fonts help URL

### Requirement: Named semantic theme registry

Themes SHALL be selected by stable name and resolved through semantic color slots, with a compiled fallback if the selected resource cannot load.

#### Scenario: Bundled themes resolve by stable name
Given a known bundled theme name
When the theme registry resolves it
Then all required semantic color slots are usable
And the requested theme name remains stable across serialization

#### Scenario: Unknown theme fails safely
Given an unknown or unreadable selected theme
When the TUI initializes
Then Alpharius is used as the effective theme
And the application remains renderable
