+++
id = "8ffb1078-1bb6-4f55-8eba-39bffa7a1151"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Dashboard — Implementing/Implemented Rendering

### Requirement: Clickable Design Tree dashboard items

Design Tree items rendered by the dashboard must expose clickable OSC 8 links when the underlying design document path is known.

#### Scenario: Design footer entry opens the design document

Given a Design Tree dashboard item with a known markdown file path
And mdserve is running for the project root
When the dashboard renders the Design Tree item in the footer or overlay
Then the item text is wrapped in an OSC 8 link
And the link target is the mdserve HTTP URL for that markdown file

#### Scenario: Design footer entry falls back to file URI

Given a Design Tree dashboard item with a known markdown file path
And mdserve is not running
When the dashboard renders the Design Tree item in the footer or overlay
Then the item text is wrapped in an OSC 8 link
And the link target is a file:// URI for that markdown file

### Requirement: Clickable OpenSpec dashboard items

Top-level OpenSpec change items rendered by the dashboard must expose clickable OSC 8 links when the change directory is known.

#### Scenario: OpenSpec change opens proposal by default

Given an OpenSpec dashboard change with a known change directory
And a proposal.md file exists in that change directory
When the dashboard renders the top-level OpenSpec change item
Then the change name is wrapped in an OSC 8 link
And the link target is the resolved URI for proposal.md in that change directory

#### Scenario: OpenSpec change stays plain when no proposal exists

Given an OpenSpec dashboard change without a proposal.md file
When the dashboard renders the top-level OpenSpec change item
Then the change name is rendered without an OSC 8 link

### Requirement: Shared URI resolver consistency

Dashboard links must use the same URI resolution rules as the view tool so markdown routes to mdserve when available and degrades gracefully otherwise.

#### Scenario: Dashboard link generation delegates to the shared resolver

Given the dashboard renders a clickable item for a known file path
When the dashboard computes the URI target
Then it uses the shared URI resolver module
And it passes the current mdserve port when available

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
