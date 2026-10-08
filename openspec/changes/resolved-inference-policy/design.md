# Resolved inference route policy — accepted design

## Status and existing authority

Proposed on 2026-10-08 against `6b276f3b8fc7e610ff61dab961d3e21778114c00`.
The user ratified the three policy defaults and authorized implementation on
2026-10-08. The [canonical node](../../../docs/resolved-inference-policy.md)
records accepted direction and stable architecture references. The deltas define
the accepted behavioral contract; implementation evidence remains separate.

Extend the existing [request-preparation contract](../unified-model-request-contract/design.md)
through `PreparedModelRequest`, the inference gateway, context assembly, and
semantic surfaces. Route selection, authentication, admission, leases, authority,
and completion keep their existing owners. This policy refines resource and
reasoning resolution after route selection; it does not select a rival route.

## Evidence ledger

### Local observations at 6b276f3

The inspected local revision is `6b276f3b8fc7e610ff61dab961d3e21778114c00`.
It was not pushed; no public permalink for it is established. The portable links
below open checkout files, which can change. Reproduce the inspected version from
this checkout with `git show <revision>:<repository-relative-path>`, for example:

```sh
git show 6b276f3b8fc7e610ff61dab961d3e21778114c00:core/crates/omegon/src/settings.rs
git show 6b276f3b8fc7e610ff61dab961d3e21778114c00:data/model-registry.json
```

These observations describe source behavior, not live provider acceptance or
account capacity. External peer citations below retain their public pinned revisions.

| Source | Observed behavior |
| --- | --- |
| [Registry](../../../data/model-registry.json) | `openai-codex:gpt-6-astra` has `contextInput=872000`, `contextOutput=128000`; its description names a 272k upstream default. |
| [Settings inference](../../../core/crates/omegon/src/settings.rs) | `infer_context_window` uses exact registry input, then route ceiling, then heuristic fallback. |
| [Selector policy and thinking](../../../core/crates/omegon/src/settings.rs) | `selector_policy` lets class/posture reduce assembly; reply and tool reserves are heuristic. `Minimal` reserves 2,000 tokens locally. |
| [Context adapter](../../../core/crates/omegon/src/loop_context.rs) | Fixed-context validation counts system and schemas plus reply reserve. `context_update` reports `conversation.estimate_tokens()` with the assembly window, excluding those mandatory inputs. |
| [Conversation estimator](../../../core/crates/omegon/src/conversation.rs) | Conversation usage uses the local chars/4-style estimate, not exact provider tokenization. |
| [Context event projection](../../../core/crates/omegon/src/tui/agent_events.rs) and [render refresh](../../../core/crates/omegon/src/tui/render.rs) | `ContextUpdated` handling computes percentage against the event window; render refresh replaces the window and thinking label from Settings without recomputing that percentage. |
| [Provider mapping](../../../core/crates/omegon/src/providers.rs) | `model_reasoning_effort` maps absent, `off`, `none`, `minimal`, and `low` to `low` for Astra. Local explicit levels are low/medium/high/xhigh/max, not ultra; unknown strings fall back to medium. |
| [Inventory](../../../core/crates/omegon/src/inference_inventory.rs) | Reasoning inventory is a capability boolean. `InventorySource::precedence`, `InventorySnapshot::build`, and `apply_offering_patch` own provenance-preserving merge and protected route identity. |
| [Prepared request](../../../core/crates/omegon/src/model_request.rs) and [route service](../../../core/crates/omegon/src/provider_route_service.rs) | Common validation, frozen options, route generation, and flow-specific evidence already exist. |
| [Command registry](../../../core/crates/omegon/src/command_registry.rs) | `context` already registers `status`; inspection can extend this surface. |

### Peer and provider evidence reviewed 2026-10-08

The comparison uses these immutable peer revisions. The summaries are original
analysis; no third-party implementation or prose is incorporated.

- **Oh My Pi, `736ce1d99aca62bab4481869d36519d79af44bb8`:**
  [context policy](https://github.com/can1357/oh-my-pi/blob/736ce1d99aca62bab4481869d36519d79af44bb8/packages/catalog/src/compat/context-window.ts),
  [Codex rules](https://github.com/can1357/oh-my-pi/blob/736ce1d99aca62bab4481869d36519d79af44bb8/packages/catalog/src/compat/rules/providers/openai-codex.kdl),
  and [thinking normalization](https://github.com/can1357/oh-my-pi/blob/736ce1d99aca62bab4481869d36519d79af44bb8/packages/catalog/src/model-thinking.ts).
  The comparison found 272k defaults for API/Codex with extended context opt-in,
  visible minimal-to-low normalization, and model-switch clamping. Its curated
  922k Codex maximum imports an API input limit; subscription entitlement remains
  unverified. Discovery reduces the Codex effort ladder/default to a boolean,
  while broader defaults favor high reasoning. These are peer choices, not norms.
- **OpenCode V2, `a67cf4c3fd2974b44ee4a17642633deef6308db2`:**
  [catalog refresh](https://github.com/anomalyco/opencode/blob/a67cf4c3fd2974b44ee4a17642633deef6308db2/packages/core/src/models-dev.ts),
  [connection transforms](https://github.com/anomalyco/opencode/blob/a67cf4c3fd2974b44ee4a17642633deef6308db2/packages/core/src/plugin/provider/openai.ts),
  [variants](https://github.com/anomalyco/opencode/blob/a67cf4c3fd2974b44ee4a17642633deef6308db2/packages/core/src/variant.ts),
  and [request preparation](https://github.com/anomalyco/opencode/blob/a67cf4c3fd2974b44ee4a17642633deef6308db2/packages/core/src/session/model-request.ts).
  Catalog refresh uses `models.opencode.ai` with a five-minute interval.
  The comparison found API 1.05M total/922k input/128k output, legacy Codex
  400k/272k, and a newer token-sharing API path retaining catalog limits.
  Invalid variant IDs are rejected, but raw settings/body values can bypass that
  check and overrides are not necessarily maximum-clamped.
- **Official Codex, `2fdf047c9631c9ed01a31b62efb7891718a931a8`:**
  [model catalog](https://github.com/openai/codex/blob/2fdf047c9631c9ed01a31b62efb7891718a931a8/codex-rs/models-manager/models.json)
  gives Astra `context_window=272000`, `max_context_window=872000`, default
  reasoning `low`, and low/medium/high/xhigh/max/ultra. The ultra description
  refers to automatic task delegation; it is not evidence for an ordinary API
  `reasoning.effort=ultra` value. Codex's
  [95% effective-window margin](https://github.com/openai/codex/blob/2fdf047c9631c9ed01a31b62efb7891718a931a8/codex-rs/models-manager/src/model_info.rs)
  is client policy, not a provider capacity fact.
- **Official API documentation**, reviewed 2026-10-08:
  [GPT-6 Astra](https://developers.openai.com/api/docs/models/gpt-6-astra.md)
  states 1,050,000 total context, 922,000 maximum input, 128,000 maximum output,
  and low/medium/high/xhigh/max reasoning. This URL is mutable and date-qualified;
  it is not an immutable source revision or proof of any OAuth entitlement.

Different UI gauge and compaction thresholds can be intentional in both peers.
The Omegon defect is specifically a percentage and displayed denominator from
different windows. A successful small request proves neither maximum capacity
nor account entitlement. Do not infer a Codex total by adding its default to an
output limit, or substitute the API's 922k input for its documented maximum.

## Accepted policy model

Names below define review vocabulary, not a new public API or service.

| Layer | Required content |
| --- | --- |
| Provider facts | Route identity; independent known/unknown input, total, output, default-window and maximum-window fields; reasoning kinds, supported sets/bounds and defaults. |
| Operator intent | Portable or concrete reasoning request, positive numeric route-scoped context target, explicit caps, posture/class constraints, and configuration origin. |
| Effective request | Captured route and metadata generations, selected working window, assembly budget, accounting, normalized reasoning, final wire enforcement, and diagnostics. |

The persisted route key includes provider, stable connection ID, authentication
class, endpoint identity, and native/deployment model identity. It distinguishes
API, OAuth, and custom endpoints even when model display names match. It contains
no credential token or secret endpoint value. Changing a key field prevents
transfer of a saved target. Routine token refresh for the same connection and
route key retains intent; captured credential/contribution generations still
govern freshness and dispatch admission. Retain selected/serving identities.

Each fact has units and meaning, source kind, source URI/revision when available,
observed/reviewed time, freshness, and declared/discovered/assumed status. Retain
source terminology when its window basis is unspecified. `Unknown` is neither
zero nor unlimited. `Configured` identifies operator input; it cannot relabel an
assumption as a provider fact. Refresh generation and content revision are distinct.

### Evidence selection and supersession

Consume the existing inventory first. Its observed layer order, highest first,
is Probe, Discovery, Session, Project, User, Organization, Embedded. Reuse this
order and existing field-specific ownership restrictions; discovery cannot
replace a previously declared route identity. Source priority does not turn
operator configuration or a successful small probe into a provider capacity fact.
Reviewed route records classify legacy `contextInput` and ceilings field by field.

The following revision-selection rules apply to policy facts:

1. Filter by the captured route's scope, semantic field, and valid generation.
   An explicitly scoped template can supply a declared fallback for that route;
   facts for another connection/authentication route cannot fill its fields.
2. Apply the inventory's established source precedence and field ownership.
   For the same source authority, scope, and field, a newer comparable publisher
   revision or source-issued observation sequence supersedes the older value.
   Revision order must be established by that source, not lexical hash ordering.
   A later local review time alone does not establish supersession.
3. Retain displaced observations as superseded provenance, outside the selected
   applicable fact set. A newer admitted route-specific discovery record can
   therefore replace an embedded declared snapshot without causing a conflict.
4. Validate the selected set across fields. Conflicting top candidates from the
   same authority with no established revision order, or inconsistent selected
   fields such as default above maximum, reject preparation with their sources.
   Do not select by numeric magnitude or silently fall back after invalid selection.

Freshness follows the source's recorded validity policy. Use admissible last-known
facts when stale, retain their bounds, and show their age; superseded or revoked
records are not reactivated merely because they are newer by local timestamp.
Staleness alone cannot increase capacity. This requires no new discovery client
or universal refresh interval.

## Capacity resolution and accounting

### Numeric target and source precedence

The core override is a positive, finite numeric token target bound to the stable
route key. A target above the advertised default is the explicit consent for
larger context. There is no boolean opt-in alternative. An optional UI `maximum`
action must materialize the currently known route maximum as a numeric target
with source revision. If the maximum is unknown, that action requests an explicit
number and creates no target. It never follows later increases automatically.

Select the first present route-scoped target in this order:

1. Supported per-request override.
2. Live-session override.
3. Project-profile route override.
4. User-profile route override.
5. Advertised default, or existing conservative host fallback labeled `Assumed`.

Retain losing targets and their origins for inspection; do not combine targets
or choose the largest. Within each existing source, preserve its established
configuration precedence and whole-type validation. An invalid winning source
rejects preparation; it cannot fall through to a valid lower-priority value.

Validate the winner against known bounds with matching semantics before applying
narrower host caps. An above-bound explicit target fails rather than clamps.
Known input/total/output constraints also apply to the resulting request. With
unknown bounds, a numeric target remains `Configured` and unverified. Generic
class/posture and legacy unscoped numeric settings are not route consent; where
applicable as host caps, they can only narrow the resolved target. Record each
reduction without rewriting stored intent. A maximum alone is not a default.

### Deterministic input budget

All quantities below are token counts, with estimator and adapter semantics
recorded. The working window inherits the route's reviewed basis; if the basis
is not established, it is `UNSPECIFIED`, not operator-invented provider metadata.

| Variable | Meaning |
| --- | --- |
| `W` | Positive finite working window after target selection and narrower host caps. Basis is `INPUT`, `TOTAL`, or `UNSPECIFIED`. |
| `I` | Estimated full provider-visible system + actual schemas + selected history/attachments. Cached input is counted once. |
| `P` | Non-overlapping input-planning and safety reserve. Actual schemas already in `I` replace their planning allocation; only an explicitly uncovered remainder enters `P`. |
| `G_h` | Existing heuristic generation allocation: reply and reasoning counted once according to their declared inclusion relationship. It is a host estimate, not a wire cap. |
| `E` | Known normalized wire-enforced cap on total generation, including every generation category that consumes the applicable total window. Otherwise absent. |
| `G` | `max(G_h, E)` when `E` is known; otherwise `G_h`, with total-generation enforcement `NotEnforced`. |
| `M_input`, `M_total` | Independent known provider input and total maxima. Unknown fields supply no candidate constraint. |

Normalize `E` through explicit provider/adapter semantics. A cap already covering
visible output and reasoning includes them once. Sum disjoint enforced caps only
when the adapter establishes that they jointly bound all relevant generation.
If a cap covers only visible output and reasoning remains unbounded or unclear,
total-generation enforcement is `NotEnforced`; retain the narrower wire field
and label total planning unverified. Validate explicit output caps against known
output bounds with the same coverage and units. Missing coverage is not evidence
of enforcement. `G_h` retains its existing constants and host inclusion rules;
unclear provider reasoning semantics remain unverified, not a fabricated mapping.

Construct the candidate set and assembly budget as follows:

```text
C_work = W       if basis = INPUT
         W - G  if basis = TOTAL
         W - G  if basis = UNSPECIFIED (Assumed host planning basis)
C = {C_work}
add M_input     to C when known
add M_total - G to C when known
B_input = min(C) - P
admit only if B_input > 0 and I <= B_input
```

Unknown bounds omit constraints; they are never recorded as infinite facts.
Resolution must supply finite `W` through intent, advertised default, or host
fallback. Reject invalid quantities, arithmetic overflow, nonpositive working
windows, and exhausted budgets. Subtraction must not wrap or saturate into a
claimed valid budget. Reserves can be zero, but cannot be negative or overlap.

Increasing `E` above `G_h` reduces every applicable total-window candidate.
Checking `I + G_h` and `E <= output maximum` separately is insufficient.
Conversely, a genuine input-only bound does not lose output capacity twice:
without a known total bound its candidate is `W`, not `W - G`. `UNSPECIFIED`
uses the conservative subtraction as host policy, without claiming a total fact.
If `G_h > E`, retain the deliberately conservative `G_h` and expose that choice;
never add the heuristic and enforced cap together.

For example, `I=90,000`, `W=M_total=100,000` with `TOTAL` basis, `P=0`,
`G_h=8,000`, and `E=20,000` gives `B_input=80,000`, so preparation rejects.
An otherwise valid request with `I=80,000` passes this budget check. These are
synthetic accounting fixtures, not provider constants or guarantees of server fit.

Preserve estimator provenance: chars/4 is heuristic and server-added prefixes
remain unknown. Keep advertised output maximum, requested output fields, `G_h`,
normalized `E`, and enforcement status distinct. Minimal's 2,000 host reserve
does not promise 2,000 actual reasoning tokens. No universal 95% multiplier or
new reserve constant is introduced. Overflow prevents dispatch; existing repair
owners may create a new preparation without silently removing mandatory input.

Warning, proactive-compaction, and emergency-compaction thresholds are distinct
named host policies over the same captured accounting basis. The effective
configured threshold must drive its named behavior; hardcoded values cannot
silently replace it. A threshold below gauge capacity is valid and inspectable.

## Reasoning resolution

Resolve a discriminated value: `Disabled`, `Enabled`, `Categorical(effort)`,
`TokenBudget(tokens)`, `Adaptive(parameters)`, or `ProviderDefault`.
Supported kinds, categorical sets, token bounds, and advertised defaults belong
to the model-route facts, not a generic reasoning boolean.

The implementation clarification accepted on 2026-10-08 adds `Enabled` for
boolean transports. Ollama Qwen-style `think: true` is Enabled, while GPT-OSS
effort strings remain Categorical. Explicit Disabled sends `think: false` on
supported routes; omission remains ProviderDefault. This represents existing
adapter semantics and does not introduce a new inference service.

Portable `Minimal` can normalize to categorical `low` through an explicit
route rule. Preserve both values and the reason. Concrete unsupported efforts
fail before transport; an unknown string must not become medium. Provider-default
with an unknown default remains `default/unknown`. A known advertised default can
be displayed with its provenance, without claiming server measurement.

**Reviewable compatibility change:** explicit `Off` fails when the route cannot
disable reasoning. This includes a saved Astra profile. Keep its stored value,
mark the selection as needing resolution, and explain supported choices. Do not
silently rewrite the profile or display disabled while sending low. Absence of
intent is provider-default, not explicit Off. Existing legacy aliases retain
their stored meaning until explicitly changed.

Token-budget and adaptive modes retain their own route semantics. Neither is
interchangeable with an effort enum or a local heuristic reserve. Codex's ultra
orchestration mode grants no ordinary effort value or delegation authority here.

## Capture, adaptation, and semantic projection

Capture the policy from the same resolved serving route and model generation
used by `PreparedModelRequest`. Assembly consumes that capture and final
preparation attaches its accounting. Normal/repair, compaction, and bounded
auxiliary adapters supply their own model, flow policy, and evidence owners.
Sessionless extraction cannot inherit the current chat model or its authority.

Model, login/connection, and metadata changes affect the next preparation under
existing route-service and execution-binding rules. In-flight work retains its
capture subject to existing cancellation/revocation. Incompatible generations at
dispatch reject the prepared request; a later attempt must resolve and prepare
again. Never silently retarget. Driver/service replacement still requires the
baseline's quiescent boundary.

Unchanged-route retries retain policy and request identity. Input repair creates
a new preparation and the existing new request/lease identity in the same step.
If final protocol adaptation or extra-body overrides would change a policy-owned
value, reject the contradiction before transport. A supported override must enter
resolution before preparation and appear in the effective snapshot. This includes
effective output limits, reasoning, native model, and context transport options.
Existing trusted lower-level bridge callers outside migrated flows remain outside
this delivery scope.

Project this data through `src/surfaces/` into TUI and ACP. The minimal header
shows effective reasoning. Usage shows estimated visible input over the final
assembly budget from one request/policy generation, with one derived percentage.
Measured provider usage is separately attributed to its request and field semantics;
it never substitutes into a different request's estimate. Before preparation,
show the available budget with usage unavailable. After model selection changes,
old usage remains labeled last-request data for its old route; the new route has
no current-request percentage yet.

Extend the registered `/context status` inspection through its existing surface
availability and provenance rules. Include requested/effective/default/maximum,
all capacity meanings and reserves, output enforcement, reasoning normalization,
source revision, age/freshness, route/policy generation, overrides, and thresholds.
This adds no mandatory large panel or dashboard.

## Migration and validation plan

Review legacy registry and route-matrix meanings per route. Preserve profile data
and existing persisted v1 facts; old records have unavailable policy evidence,
not fabricated historical snapshots. A later need for new durable evidence must
use an explicit additive, versioned contract review, not mutate v1 payloads.

Implementation uses an additive live `InferencePolicyProjection` DTO, not a new
durable session record. Old session-v1 records have no policy evidence. Existing
credential JSON gains optional `connectionId` and `credentialGeneration` metadata
on authorized credential writes. New login/import gets a connection ID; refresh
preserves that ID and changes the credential generation. Legacy records remain
readable without fabricated connection history. Native Codex also binds account
identity, and manifest routes bind endpoint/secret-reference identity. Opaque
endpoint hashes exclude endpoint secrets from inspection.

The API Astra 272k working default is explicitly a host assumption; its input,
total and output maxima retain their documented independent meanings. Codex's
legacy `contextOutput` is preserved but is not promoted into a verified output
fact from an API source. Its reviewed maximum/default window basis remains
unspecified. Unclassified registry routes retain conservative, labeled fallbacks.

Operator controls are `/context status`, `/context capacity <tokens|maximum|reset>`,
and `/context reasoning <provider-default|off|effort|budget:N|adaptive>`.
Capacity and reasoning commands change live-session intent. Profile
`routeContextTargets` stores explicit numeric targets using the route object from
inspection. Profile precedence remains project before user, with request/session
targets taking priority. Existing unscoped `contextWindow` data is preserved and
does not authorize enlarged context.

Use offline fact and fake-bridge fixtures for Codex/API Astra, Anthropic
token-budget/adaptive modes, unknown cloud routes, and local transport caps.
Cover stale metadata, invalid overrides, full-input overflow, model/connection
switches, retry/repair identity, compaction, and sessionless models. No credentialed
boundary stress test is needed. Final live verification is parent-owned after
automated reviews, using the same recorded interactive terminal.

The [tasks](tasks.md) sequence this work after policy acceptance. Structural
OpenSpec validation checks artifacts, not behavior. Native task/test registration,
ledger reconciliation, and archival have not run; the existing
[closure concern](../../../docs/lifecycle-closure-reconciliation.md) remains applicable.
