+++
id = "afbc25ef-f6f1-4c37-996f-7cfb7f808f89"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# dashboard/layout

## Requirement: Wide raised mode uses a full-width workspace header

The raised dashboard MUST separate workspace identity from the body by using a full-width header area rather than letting git branches flow directly into the Design Tree section.

### Scenario: Branch tree is structurally separated from body content
- **Given** the raised dashboard is rendered on a wide terminal
- **When** git branch information is present
- **Then** the top of the raised box renders branch/workspace identity as a distinct header band
- **And** body sections begin beneath that header instead of immediately continuing the branch tree narrative inline

## Requirement: Wide raised mode is design-dominant by default

The raised dashboard MUST treat Design Tree as the primary workspace when implementation activity is sparse or absent.

### Scenario: No active implementation work
- **Given** the raised dashboard is rendered on a wide terminal
- **And** there are no active implementation changes or cleave activity requiring large presentation
- **When** the body layout is composed
- **Then** Design Tree occupies the primary work area
- **And** implementation does not permanently reserve half the body as empty space

## Requirement: Implementation is contextual rather than permanently symmetric

Implementation/OpenSpec/Cleave state MUST appear contextually in a secondary rail or docked section when active.

### Scenario: Active implementation work is present
- **Given** the raised dashboard is rendered on a wide terminal
- **And** there is active OpenSpec or Cleave work to show
- **When** the body layout is composed
- **Then** implementation information appears in a contextual support area
- **And** Design Tree remains the primary workspace rather than collapsing into a permanent 50/50 split

## Requirement: Responsive collapse preserves semantic reading order

Reflow across width tiers MUST preserve the same conceptual order: workspace/work first, telemetry later.

### Scenario: Medium-width terminal reflow
- **Given** the same dashboard state is rendered on a medium-width terminal
- **When** the raised layout collapses from the wide arrangement
- **Then** the layout preserves a clear workspace-first reading order
- **And** it does not revert to an arbitrary design-vs-implementation split solely because width is smaller
