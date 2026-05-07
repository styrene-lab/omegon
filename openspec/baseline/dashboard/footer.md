+++
id = "bcb423fc-d5e0-438e-b81b-6063f949e8a2"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# dashboard/footer

### Requirement: Raised dashboard pins operator metadata at the bottom

Raised dashboard mode MUST keep operator-critical metadata visible in a fixed bottom block even when upper dashboard sections grow.

#### Scenario: raised dashboard keeps bottom metadata visible
Given the dashboard is in raised mode
And design-tree, OpenSpec, cleave, or recovery sections above it expand
When the footer is rendered
Then the bottom metadata block still includes context/model/thinking and memory-oriented operator information
And upper content sections absorb truncation or compression pressure before that bottom block disappears

#### Scenario: raised dashboard still includes a compact-mode hint in the pinned block
Given the dashboard is in raised mode
When the footer is rendered
Then the operator still sees the compact/raise-lower hint in the reserved bottom block
And the hint is not pushed below duplicate generic footer rows

### Requirement: Raised dashboard does not render a duplicate generic footer block

Raised mode MUST not append generic footer rows that duplicate dashboard metadata already shown in the custom dashboard footer.

#### Scenario: raised mode omits duplicate compact context footer rows
Given the raised dashboard already renders a context gauge and model metadata
When the footer is rendered
Then it does not append a second compact context summary row that repeats the same information below the dashboard block
And the result reads as one coherent dashboard rather than two stacked footers

#### Scenario: raised mode preserves only unique footer data if any remains
Given the existing generic footer renderer contains information not already shown in the dashboard
When raised mode is rendered
Then only uniquely useful operator information may remain
And duplicated pwd/context/status rows are removed from the raised dashboard output

### Requirement: Recovery expands only when actionable

Recovery information in the dashboard MUST expand only when the current recovery state is actionable enough to deserve dedicated space.

#### Scenario: non-actionable recovery does not consume two raised rows
Given recovery state exists only as background telemetry or already-resolved status
When the raised dashboard is rendered
Then recovery does not consume the full two-line expanded recovery section
And recovery is instead omitted or collapsed to a compact indicator

#### Scenario: actionable recovery still expands visibly
Given recovery state indicates escalation, active cooldown pressure, retry exhaustion, or a meaningful model/provider switch event
When the raised dashboard is rendered
Then recovery expands into a clearly visible section
And the operator can still see why attention is needed

### Requirement: OpenSpec rows are compact and visually coherent

OpenSpec rows in the raised dashboard MUST present change status clearly without awkward spacing or excessive inline separators.

#### Scenario: OpenSpec header and rows use tighter formatting
Given the raised dashboard renders the OpenSpec column
When one or more changes are present
Then the OpenSpec header and change rows use compact spacing
And the change name remains visually primary over stage/progress chrome

#### Scenario: OpenSpec rows reduce separator noise
Given the raised dashboard renders OpenSpec change rows
When progress and stage metadata are displayed
Then the row format uses only the minimum separators needed for clarity
And the output avoids looking padded or fragmented by repeated inline punctuation
