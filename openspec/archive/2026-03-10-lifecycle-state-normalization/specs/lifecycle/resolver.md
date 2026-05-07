+++
id = "e33f5ace-784a-4ecd-87e4-d80e1b10449b"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# lifecycle/resolver — Delta Spec

## ADDED Requirements

### Requirement: Canonical lifecycle resolver produces shared change summaries

pi-kit MUST provide a canonical lifecycle resolver that computes shared OpenSpec/design lifecycle summaries from one code path instead of re-deriving overlapping truth separately in each consumer.

#### Scenario: resolver computes shared lifecycle summary for a change
Given an OpenSpec change with tasks, assessment state, and design binding information
When a consumer requests its lifecycle summary
Then the resolver returns a normalized object containing stage, verification substate, archive readiness, binding status, task counts, and assessment freshness
And those fields are derived from one canonical implementation path

#### Scenario: resolver preserves existing coarse stage semantics
Given a task-complete change with a concrete verification substate
When the canonical lifecycle resolver computes its summary
Then the coarse stage remains `verifying`
And the finer-grained verification substate is attached without changing the historical stage contract

### Requirement: OpenSpec status surfaces consume the canonical lifecycle resolver

OpenSpec operator-facing status and archive-readiness paths MUST consume the canonical lifecycle resolver rather than independently recomputing overlapping lifecycle truth.

#### Scenario: get and status surfaces agree about lifecycle details
Given an active change with a current assessment and design binding
When OpenSpec renders both status-list and get-detail views
Then both surfaces report the same stage, verification substate, and archive readiness
And neither surface uses a divergent local interpretation of lifecycle state

#### Scenario: archive gating and status reporting share the same readiness rule
Given a change is archive-ready according to the canonical lifecycle resolver
When OpenSpec checks whether archive should proceed
Then the archive gate uses the same readiness outcome reflected in status reporting
And a change blocked by stale assessment or missing binding is reported consistently before archive is attempted

### Requirement: Dashboard and design-tree bindings consume canonical lifecycle state incrementally

Dashboard and design-tree lifecycle views MUST be able to consume the canonical lifecycle resolver for shared binding/readiness truth without requiring a full mutable-state rewrite.

#### Scenario: design-tree binding truth matches lifecycle resolver output
Given a design node is bound to an OpenSpec change by explicit field or accepted fallback rule
When design-tree reports its lifecycle metadata
Then the bound-to-OpenSpec truth matches the canonical lifecycle resolver
And design-tree does not contradict OpenSpec about whether the change is lifecycle-bound

#### Scenario: dashboard-facing lifecycle state uses the same readiness summary
Given dashboard state includes OpenSpec lifecycle information for active changes
When the dashboard extension publishes that state
Then it consumes the canonical lifecycle resolver output for shared readiness and verification-substate fields
And it does not maintain a separate ad hoc derivation for those same concepts
