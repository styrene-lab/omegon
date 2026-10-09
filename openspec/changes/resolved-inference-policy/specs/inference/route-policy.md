# Inference route policy — Delta Spec

Accepted requirements. These add policy semantics to the existing
[preparation contract](../../../unified-model-request-contract/specs/inference/preparation.md)
without replacing its authority or flow requirements. Numbers in the Astra
scenarios are dated 2026-10-08 fixtures, not permanent model invariants.

## ADDED Requirements

### Requirement: Route facts retain identity semantics and provenance

Resolution must use the actual serving route, distinguished by provider, stable
connection ID, authentication class, endpoint identity, and native/deployment
model. This persisted route key must contain no credential token. Routine token
refresh with the same key preserves saved intent; captured generations still
govern freshness and dispatch. A different key must not inherit the target.
Each capacity and reasoning fact must retain units, semantic meaning, source
kind, available URI/revision, observed/reviewed time, freshness, and evidence
status. Input, total, output, default window, and maximum window are independent
known/unknown fields. Unknown must not mean zero, unlimited, or verified.
Configured values and host assumptions must not become provider facts or
account-entitlement claims.

Fact selection must first enforce route/field scope and valid generation, then
reuse established inventory source precedence and field ownership. The current
source order is Probe, Discovery, Session, Project, User, Organization, Embedded,
highest first; this order must not grant evidence semantics that a source lacks.
Within one source authority and scope, newer comparable publisher revisions or
source-issued observation sequences supersede older observations. Local review
time alone must not establish supersession. Superseded observations remain
provenance, outside the selected applicable fact set. Unordered conflicting top
observations from the same authority and inconsistent fields within the selected
set must reject preparation. Selection must not use the largest value, borrow
cross-route facts, or silently fall back after selecting an invalid set.

#### Scenario: API facts cannot fill an OAuth route by model name
Given API Astra facts of 1,050,000 total, 922,000 input, and 128,000 output tokens
And separate Codex Astra facts of 272,000 default and 872,000 maximum window
When the Codex route policy is resolved
Then it retains the Codex default and maximum with their source meanings
And API input or total values do not fill unknown Codex fields
And no account entitlement or total equal to default plus output is inferred

#### Scenario: Unknown route uses labeled conservative assumptions
Given a compatible unknown cloud or local route with an existing conservative host fallback
And a local transport cap is known when that route supplies one
When its policy is resolved
Then the fallback is labeled Assumed with unknown provider fields preserved
And the effective budget respects every known transport cap
And a prior successful small request does not mark the assumed capacity verified

#### Scenario: Stale evidence retains its bounds
Given stale last-known route facts with recorded source revisions and review times
When the next policy is resolved
Then admissible stale facts retain their bounds and expose their age and freshness
And staleness alone does not increase capacity

#### Scenario: Contradictory applicable facts block preparation
Given selected facts whose comparable default exceeds maximum, or conflicting top observations from one authority with no established revision order
When the next policy is resolved
Then preparation is rejected before transport with both source revisions and conflicting fields
And resolution does not silently select the larger value

#### Scenario: New applicable metadata supersedes a declared snapshot
Given an embedded declared route snapshot and a newer valid exact-route Discovery observation for the same capacity field
And the observed value differs from the declared value
When facts are selected through established inventory precedence
Then the Discovery observation supplies that field and the old value remains superseded provenance
And the value change alone does not cause conflict rejection
And protected route identity and other selected fields must still validate

### Requirement: Working capacity separates default ceiling and explicit intent

The core override must be a positive finite numeric token target bound to the
stable route key. Select the first present target in this order: supported
per-request override, live-session override, project-profile route override,
user-profile route override, then known advertised default or labeled conservative
host fallback. Preserve established precedence and whole-type validation within
each source. Retain losing targets as provenance, not combined limits. An invalid
winner must reject without falling through. A documented maximum alone must not
become the default.

Larger-than-default operation requires this numeric route target. An optional UI
maximum action must materialize the currently known maximum and its revision as
a positive numeric target. Unknown maximum must produce a request for an explicit
number, without creating a target. Boolean opt-in is not a core policy value.
Generic class/posture and legacy unscoped numeric settings must not imply route
consent; applicable host caps may only narrow the resolved target.

Validate explicit targets against known bounds with the same semantic meaning
before host narrowing. All known input, total, output, and transport constraints
also apply to the resulting request. Malformed, inconsistent, or above-bound
intent must fail before transport with the field, bound, source, and corrective
action. Unknown bounds allow Configured numeric intent without implying
verification. Narrower host caps must retain intent and explain the reduction.

#### Scenario: Maximum does not become the default working window
Given the Codex fixture advertises default 272,000 and maximum 872,000 tokens
And the operator selects a generic massive class or a legacy unscoped numeric setting without a numeric route override
When the working window is resolved
Then it is no greater than 272,000 before applicable reserves and narrower caps
And inspection distinguishes default, ceiling, requested class, and effective budget

#### Scenario: Explicit larger target and narrower caps
Given the Codex fixture and an explicit route-scoped target of 600,000 tokens
And a narrower host working cap of 400,000 tokens
When the policy is resolved
Then the effective working window is 400,000 before reserves
And the original numeric target, source, known maximum, and cap reduction are retained
And the target does not transfer to another connection or model

#### Scenario: Highest priority numeric target wins or fails without fallback
Given a project-profile route target of 600,000, user-profile target of 500,000, and known maximum of 872,000 tokens
And no narrower host cap applies
When supported per-request targets of 300,000 and 900,000 are resolved as separate fixtures
Then the 300,000 target wins and lower-priority targets remain source provenance
And the 900,000 target rejects before transport rather than falling back to 600,000
And targets are neither maximized nor silently clamped

#### Scenario: Maximum action must materialize a known numeric target
Given one route fixture has known maximum 872,000 and another has unknown maximum
When the operator selects the optional maximum action for each fixture
Then the known fixture materializes numeric target 872,000 with route key and source revision
And the unknown fixture requests an explicit number and creates no target
And a later metadata increase does not silently increase the materialized target

#### Scenario: Invalid overrides fail without silent clamping
Given an explicit target is zero, malformed, or exceeds a known route bound
When preparation validates the target
Then no provider stream is opened
And diagnostics identify the invalid field and a corrective action
And neither profile data nor provider facts are silently changed

#### Scenario: Unknown capacity does not disguise a configured target
Given a compatible route has no documented default or maximum window
And an explicit positive route target passes every known transport constraint
When its policy is resolved
Then the target has Configured provenance and unverified capacity status
And unknown provider bounds remain inspectable as unknown

### Requirement: One accounting policy validates complete visible input

Preparation must use the following quantities in tokens, retaining estimator
provenance, units, and uncertainty about hidden server input:

- `W`: positive finite working window after target selection and host narrowing,
  with the route's reviewed `INPUT`, `TOTAL`, or `UNSPECIFIED` basis.
- `I`: full visible estimated system + actual schemas + selected history/attachments.
  Cached input is counted once.
- `P`: non-overlapping input-planning and safety reserve. Actual schemas in `I`
  replace their planning allocation; only an explicitly uncovered remainder enters `P`.
- `G_h`: existing heuristic generation allocation, counting reply/reasoning once
  according to declared host inclusion relationships, not claimed provider consumption.
- `E`: known normalized wire-enforced total-generation cap. Adapter semantics
  must identify all generation categories covered, including applicable reasoning.
- `M_input` and `M_total`: independent known input and total maxima.

Use `G = max(G_h, E)` when `E` is known; otherwise use `G = G_h` and report
total-generation enforcement as NotEnforced. Preserve a larger heuristic as
deliberate, inspectable conservatism; never sum the heuristic and wire cap.
Shared reasoning/output caps include reasoning once. Disjoint enforced caps
may be combined only when declared adapter semantics establish a bound for all
generation consuming the total window. A partial cap with unknown reasoning
coverage must remain visible as a partial wire field; it does not establish `E`.
Report that total planning as unverified rather than claiming enforcement.

Build the candidate set and final input assembly budget exactly as follows:

```text
C_work = W       for INPUT
         W - G  for TOTAL
         W - G  for UNSPECIFIED, labeled Assumed host planning basis
C = {C_work}
include M_input     when known
include M_total - G when known
B_input = min(C) - P
admit only if B_input > 0 and I <= B_input
```

Unknown maxima omit constraints; they are not infinite facts. A finite `W` must
come from intent, advertised default, or conservative host fallback. Reject
invalid quantities, arithmetic overflow, nonpositive windows, or exhausted
budgets; subtraction must not wrap or saturate into a claimed valid budget.
Reserves may be zero but must not be negative or overlapping. Do not subtract
generation from a genuine input-only bound unless a separate known total bound
also constrains the candidate set. An unspecified basis must not become a
provider total fact.

Explicit wire output caps must satisfy known output bounds with matching coverage
and units. A larger normalized `E` must reduce applicable total-window candidates;
separate checks of `I + G_h` and the output cap are insufficient. Advertised output
maximum, requested fields, heuristic allocation, `E`, and enforcement status must
remain distinct. Preserve existing reserve constants initially; no universal 95%
margin is introduced. Named warning, proactive, and emergency thresholds must use
this captured policy basis and their effective configured values.

#### Scenario: History makes otherwise valid fixed overhead overflow
Given system and schemas plus reserves fit the route window
And the assembled history makes full visible input exceed the assembly budget
When the request is prepared
Then dispatch is rejected with the exceeded budget and estimated components
And mandatory input is not silently truncated
And any repair must produce a new validated preparation

#### Scenario: Shared output and cached input are counted once
Given an estimate includes cached history, actual schemas, and reasoning within an output allocation
And the transport has no output-limit field for this request
When preparation calculates capacity
Then cached history and actual schemas are each counted once
And schema planning reserves cover only an explicitly uncovered remainder
And reasoning already included in output is not reserved a second time
And output maximum, local reserve, and NotEnforced wire output remain distinct

#### Scenario: Enforced output cap reduces total-window input fit
Given W and M_total are 100,000 with TOTAL basis, G_h is 8,000, E is 20,000, and P is zero
And the declared output maximum permits E and all other constraints pass
When otherwise identical requests with I of 90,000 and 80,000 are validated
Then B_input is 80,000 for both requests
And the 90,000 request is rejected before transport while the 80,000 boundary request passes the budget check
And the lower heuristic alone cannot justify accepting the 90,000 request

#### Scenario: Genuine input-only bound does not subtract output
Given W and M_input are 100,000 with INPUT basis and M_total is unknown
And G_h is 8,000, E is 20,000, P is zero, and all explicit output bounds pass
When a request with I of 100,000 is budget-validated
Then B_input is 100,000 and the input budget check passes
And no invented total bound or extra output subtraction reduces it to 80,000

#### Scenario: Unspecified window basis uses conservative host accounting
Given W is 100,000 with UNSPECIFIED basis and provider input and total maxima are unknown
And G_h is 8,000, E is 20,000, and P is zero
When a request with I of 90,000 is budget-validated
Then B_input is 80,000 and preparation is rejected
And the subtraction has Assumed host planning provenance without creating a provider total fact

#### Scenario: Heuristic conservatism and incomplete enforcement stay inspectable
Given a TOTAL working window of 100,000, P of zero, and G_h of 24,000
And one adapter fixture establishes E of 20,000 while another exposes only a visible-output cap with unknown reasoning coverage
When each fixture calculates its input budget
Then G is 24,000 and B_input is 76,000 in both fixtures
And the first reports a known E below its deliberately larger heuristic
And the second reports total-generation NotEnforced and unverified planning while retaining its partial wire cap
And neither sums the wire cap with G_h nor fabricates reasoning coverage

#### Scenario: Named configured threshold controls compaction
Given proactive compaction has an effective threshold below gauge capacity
And warning and emergency thresholds are separately named in the same policy
When estimated pressure reaches the proactive threshold
Then proactive compaction is requested under its existing admission owner
And inspection identifies the threshold and policy basis that caused the request
And no hardcoded replacement silently ignores the effective threshold

### Requirement: Reasoning preserves intent and resolves supported route semantics

Reasoning must distinguish Disabled, Enabled, Categorical, TokenBudget, Adaptive, and
ProviderDefault. Route-specific sets, bounds, and defaults determine support.
Portable normalization requires an explicit rule and retained reason. Concrete
unsupported intent, including Off when disabling is unsupported, must reject
before transport. Saved incompatible intent must remain stored and be marked as
needing resolution. An unknown provider default must remain default/unknown.
Orchestration modes must not become effort values or grant delegation authority.

#### Scenario: Portable Minimal normalizes visibly to low
Given portable Minimal and the Astra rule mapping Minimal to categorical low
When reasoning is resolved
Then requested Minimal, effective low, and the normalization reason are preserved
And the provider receives low and semantic UI reports low
And any retained 2,000-token host reserve is labeled heuristic, not actual reasoning tokens

#### Scenario: Saved Off requires resolution on Astra
Given a saved profile explicitly requests Off and the selected Astra route cannot disable reasoning
When the next request is prepared
Then it is rejected before transport with supported choices and needs-resolution feedback
And the saved value remains Off until the operator changes it
And no UI reports disabled while a request sends low

#### Scenario: Unsupported concrete effort cannot hide behind an override
Given an Astra request contains explicit effort ultra or another unsupported concrete string
When reasoning is validated through any supported options or override path
Then transport is rejected rather than coercing the value to medium or max
And Codex automatic-delegation metadata does not authorize the value

#### Scenario: Noncategorical and unknown-default modes retain their meaning
Given route fixtures for bounded Anthropic token budgets, Anthropic adaptive reasoning, a disable-capable local model, and an unknown provider default
When each route resolves a supported request in its own reasoning kind
Then token-budget bounds are enforced, adaptive parameters remain adaptive, and explicit disabled remains disabled
And the unknown default is shown as default/unknown
And no kind is fabricated from a capability boolean or a local reserve

#### Scenario: Boolean reasoning is distinct from categorical effort and omission
Given explicit Ollama adapter rules for boolean Qwen-style reasoning and categorical GPT-OSS effort
When Minimal, supported Off, and absent intent are prepared for these routes
Then Qwen-style Minimal resolves to Enabled and sends think true
And GPT-OSS Minimal resolves to categorical low
And supported Off sends think false while absence remains ProviderDefault
And a contradictory late think override rejects before transport

### Requirement: Prepared requests bind one policy to their captured route

Included normal, repaired, compaction, and bounded auxiliary requests must carry
an immutable resolved policy from the same route/model/contribution generation
as their prepared inputs and options. Record metadata revision and policy inputs.
Model, login/connection, and metadata changes apply to the next preparation under
existing binding rules. In-flight requests retain capture subject to existing
revocation/cancellation. Incompatible generation at dispatch must reject the
prepared request; a new attempt must resolve and prepare again.

Flow-specific authority, no-tools constraints, deadlines, collectors, and evidence
remain governed by the preparation contract and existing baselines. Unchanged
transport retry retains capture; repaired input creates new request/lease identity.
Final adaptation must agree with resolved values. Contradictory late overrides
must reject before transport; supported overrides enter resolution before capture.

#### Scenario: Connection and metadata changes do not mutate in-flight policy
Given a prepared request under route generation A and metadata revision R
And model selection, login/connection, or admissible metadata changes during its execution
When the next request is prepared
Then it resolves from the next request's admitted route and current metadata
And the in-flight request retains A and R subject to existing revocation and cancellation
And driver or route-service replacement still obeys quiescent migration rules

#### Scenario: Routine token refresh preserves intent for the stable route
Given a saved numeric target for a stable connection, endpoint, authentication class, and native model
And routine token refresh changes credential generation without changing that route key
When the next request is prepared
Then it retains the route-scoped target and captures the newly admitted generation
And no token value appears in the persisted route key
And a different connection or model key would not inherit that target

#### Scenario: Incompatible generation cannot silently retarget dispatch
Given a prepared policy whose captured route generation is incompatible at dispatch
When dispatch revalidates the request
Then no provider stream opens for that prepared request
And diagnostics require fresh resolution and preparation rather than silent retargeting

#### Scenario: Compaction and sessionless auxiliary models have independent policy
Given chat, captured compaction, and bounded auxiliary inference select different models
When the compaction and auxiliary requests are prepared
Then each uses facts and effective values for its own captured serving model
And each retains zero tools and its existing deadlines, result limits, and evidence owner
And sessionless source attribution acquires no chat policy or session authority

#### Scenario: Retry and repair preserve their distinct identities
Given an unchanged-route retry and a history-repair request within one logical step
When each is admitted through existing preparation owners
Then the unchanged retry reuses its request, lease, and policy snapshot
And the repair has new request and lease identities with newly validated accounting
And the repair remains in the existing step without widening authority

#### Scenario: Late protocol override cannot contradict a captured policy
Given a prepared request with validated reasoning, output accounting, context options, and native model
And a late adapter or extra-body override contradicts a policy-owned value
When final protocol adaptation runs
Then the contradiction is rejected before transport
And a supported override requires resolution and capture of its effective value before dispatch
