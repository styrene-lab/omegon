+++
id = "2daaccd2-cbd7-41b7-993e-d68cba70b781"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# dashboard — Delta Spec

## ADDED Requirements

### Requirement: Intelligent truncation preserves high-value dashboard semantics

Dashboard footer and overlay rows that include long titles, branches, stage labels, or counts must truncate predictably so operators retain the most important information instead of seeing awkward clipping in the middle of composite status lines.

#### Scenario: Raised footer trims long Design Tree and OpenSpec rows without losing status and progress
Given a raised dashboard footer with long design node titles, long branch names, and long OpenSpec change names
When the footer renders in a constrained width
Then each row preserves its leading status icon and primary label
And branch, stage, and progress metadata are truncated only after the primary label is shown
And the final rendered row fits within the available width

#### Scenario: Compact footer trims trailing metadata before primary summaries
Given a compact dashboard footer with multiple sections and a long model identifier
When the footer renders in a constrained width
Then design, OpenSpec, cleave, and context summaries remain visible before trailing driver or model metadata
And low-priority hint text is truncated before primary dashboard summaries

#### Scenario: Overlay rows trim nested metadata without losing open targets
Given the interactive dashboard overlay contains long item labels and expandable metadata rows
When the overlay renders within its content width
Then openable rows still show the open marker and main label
And secondary metadata is ellipsized to fit the content width

### Requirement: Deep dashboard view uses a screen-wide blocking layout

When the operator asks for a deeper dashboard view, the explicit interactive dashboard command must use a wide blocking overlay sized for inspection rather than a narrow side panel.

#### Scenario: Interactive overlay opens as a wide centered inspection view
Given the operator opens the interactive dashboard view
When the overlay is rendered on a terminal with sufficient width
Then the overlay is centered instead of right-anchored
And it uses most of the terminal width
And it keeps a bounded margin so the border remains visible

#### Scenario: Non-capturing panel remains compact for glanceable use
Given the operator toggles the non-capturing dashboard panel
When the panel is rendered
Then it may remain narrower than the blocking inspection view
And the compact footer continues to be the primary always-visible summary
