# Inference policy projection — Delta Spec

Accepted requirements. The [route-policy delta](route-policy.md) owns
resolution; projections consume its results through shared semantic surfaces.

## ADDED Requirements

### Requirement: Semantic context and reasoning displays use one identified snapshot

The minimal header must report effective reasoning, including default/unknown
when appropriate. Context count, denominator, and percentage must derive from
one request/policy generation. The gauge uses estimated visible input over its
final assembly budget, labeled as an estimate. Measured provider usage must retain
its request identity, timing, and field semantics separately. Renderers must not
replace a denominator or effort from global Settings after snapshot projection.
Before a prepared estimate exists, show budget with usage unavailable. A route
change must invalidate current usage; historical usage stays explicitly attributed
to the last request and its old route.

#### Scenario: A smaller assembly window cannot acquire a registry denominator
Given a prepared snapshot estimates 50,000 visible input tokens against a 100,000-token assembly budget
And the registry advertises a larger route window
When TUI or ACP projects current context
Then the displayed estimate is 50,000 of 100,000 and its percentage is 50%
And effective effort and usage belong to that same snapshot
And registry refresh during rendering cannot replace only the denominator

#### Scenario: Startup and model switch do not fabricate current usage
Given no request has been prepared for the selected model
And there may be usage from a previous model's request
When the operator views context status
Then the selected model shows its available budget with current usage unavailable
And any retained old usage is labeled last-request data with its own route and generation
And no zero-percent full-request estimate or old-model percentage is presented as current

#### Scenario: Provider usage does not overwrite another request estimate
Given a measured provider usage report for request A arrives after request B is prepared
When semantic usage is updated
Then measured usage remains attributed to A with its field meanings
And B retains its own estimated visible input, budget, and percentage
And cache counters are not added again to an inclusive input count

### Requirement: Existing context inspection explains effective policy

The existing registered `/context status` surface must expose requested,
effective, default, maximum, reserves, output enforcement, reasoning kind and
normalization, route/model generation, source revision, age/freshness, assumptions,
overrides, and named thresholds. Unknown values must remain explicit. Inspection
must include the working-window basis, `I`, `P`, `G_h`, known or absent `E`, `G`,
and `B_input`, with cap coverage and enforcement status. It must distinguish
winning/losing numeric targets and selected/superseded fact evidence. Inspection
must respect existing command availability/provenance and redact credentials and
request content. A lower compaction threshold than gauge capacity is valid when
both are named and use the same captured accounting basis. No new mandatory
panel is required.

#### Scenario: Operator can explain a normalized request and early compaction
Given Minimal resolved to low and a host threshold triggers before the gauge reaches capacity
When the operator invokes the existing context status command
Then inspection reports the requested and effective effort with its normalization reason
And it reports default, ceiling, working target, final assembly budget, reserves, and the triggering threshold
And it identifies the snapshot, sources, age, overrides, and unknown or assumed fields
And the minimal header can remain compact without concealing an inconsistent denominator
