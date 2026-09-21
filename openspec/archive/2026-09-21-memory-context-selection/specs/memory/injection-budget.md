# Injection budget delta

## MODIFIED Requirements

### Requirement: Memory injection stays within a tighter routine-turn budget

Routine memory SHALL use a configurable memory-specific token cap bounded by the
host allocation, rather than a percentage-based full dump. All emitted formatting
and evidence metadata SHALL count toward the budget.

#### Scenario: Multilingual content respects token allocation
Given multilingual facts, code identifiers, headings, and provenance metadata
When a memory block is packed into its allocated token budget
Then the emitted block's accounted token count does not exceed the allocation
And a zero budget produces no memory block

#### Scenario: Oversized candidate does not starve a smaller fact
Given the first candidate cannot fit and a later relevant fact can fit
When packing executes
Then the later fact is considered and included if eligible
And truncation does not emit a partial misleading claim

### Requirement: Low-value additive memory is conditional

Episodes, global facts, and structural filler SHALL require relevance and remaining
budget. Routine low-signal turns SHALL avoid unrequested cross-scope expansion.

#### Scenario: Low-signal turn has no relevant episode
Given a short turn without relevant episodic or cross-project evidence
When ambient memory is selected
Then irrelevant recent episodes and global facts are omitted
And only justified core or pinned facts consume memory budget

### Requirement: Memory telemetry remains operator-auditable

Selection telemetry SHALL expose selected IDs/counts, exclusion reasons, token
accounting method, budget exhaustion, and retrieval degradation through existing
inspection surfaces without logging fact contents at info level.

#### Scenario: Operator inspects a constrained selection
Given selection excluded facts for budget, applicability, and lifecycle status
When the operator inspects the last memory selection
Then the report distinguishes those reasons and the selected evidence handles
And it identifies whether token accounting is exact or conservative
