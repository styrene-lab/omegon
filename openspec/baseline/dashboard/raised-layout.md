# dashboard/raised-layout

### Requirement: Raised dashboard uses explicit responsive card layouts

The raised dashboard footer must use explicit layout tiers rather than relying on truncation-only degradation.

#### Scenario: Wide terminals use horizontal card regions
Given the raised dashboard is rendered in a wide terminal
When stable footer state is available
Then the lower dashboard uses explicit horizontal card regions for context, model topology, memory, and runtime/system information
And it does not fall back to the old vertically stacked HUD section layout as its primary presentation

#### Scenario: Narrow terminals retain intelligible summaries
Given the raised dashboard is rendered in a narrow terminal
When width pressure increases
Then the dashboard compresses card content before dropping categories
And context plus model topology remain visible as summary cards

### Requirement: Raised dashboard clarifies model topology

The persistent raised footer must distinguish model roles rather than flattening them into a single ambiguous status line.

#### Scenario: Operators can distinguish model roles at a glance
Given the raised dashboard shows current system/model state
When an operator scans the footer
Then they can distinguish the session driver from embeddings, extraction, and fallback/offline state
And role labels are consistent with the focused dashboard overlay terminology
