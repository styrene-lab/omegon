# Context selection delta

## ADDED Requirements

### Requirement: Context selection prioritizes applicable task evidence

Ambient selection SHALL use task relevance and applicability before section
presentation order. Current-context eligibility SHALL match the retrieval contract.

#### Scenario: Relevant constraint survives an Architecture flood
Given many unrelated Architecture facts and one applicable constraint matching the task
When a bounded memory context is selected
Then the matching constraint is included ahead of unrelated Architecture filler
And below-floor or known-inapplicable facts cannot bypass eligibility by section

### Requirement: Memory context surfaces share selection semantics

Standalone provider context, live ambient context, and request_context memory packs
SHALL use the same eligibility and packing policy for equivalent declared inputs.

#### Scenario: Equivalent inputs across adapters
Given the same task, candidates, pins, scope, policy, and token budget
When each memory context adapter selects evidence
Then selected IDs and exclusion reasons agree
And adapters do not implement independent decay or applicability rules

### Requirement: Pins change priority without duplicating or reviving facts

Selection SHALL deduplicate pinned and recalled identities. Inactive or inapplicable
pins SHALL be explicitly reported or resolved to labeled replacements.

#### Scenario: Pinned fact is also retrieved
Given an eligible pinned fact appears in the retrieved candidates
When the memory block is selected
Then the fact appears once and is accounted for once

#### Scenario: Pinned fact was superseded
Given a pinned fact has an active applicable replacement
When current context is selected
Then the obsolete fact is not injected as current guidance
And the pin resolution identifies its replacement

### Requirement: Selection cache invalidates on semantic input changes

Cache reuse SHALL preserve task, scope, lifecycle state, policy, and budget validity.

#### Scenario: Task changes before TTL expiry
Given a cached injection is within its TTL and the task changes to another component
When the next memory selection is requested
Then relevance is reevaluated for the new task

#### Scenario: Selected fact is archived before TTL expiry
Given a cached injection includes a fact that is subsequently archived
When the next current context is assembled
Then the archived fact is removed even if the old injection TTL has not expired

#### Scenario: Unchanged turn reuses bounded selection
Given task, scope, pins, memory version, policy, budget, and eligibility remain unchanged
When another selection is requested before cache expiry
Then the cached selection is reusable
And the selector does not rescan the entire fact store
