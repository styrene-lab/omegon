+++
id = "5f83a73e-d20a-4e43-b22a-d6bac7c98f82"
kind = "document"
title = "Runtime Identity, Authorization, Persona, Posture, and Resource Envelope Stack"
status = "active"
tags = ["architecture", "identity", "rbac", "persona", "posture", "runtime"]
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
date = "2026-04-11"
+++

# Runtime Identity, Authorization, Persona, Posture, and Resource Envelope Stack

## Why this exists

The harness is accumulating several distinct kinds of runtime state:
- identity and trust
- authorization and capability gating
- persona and mind selection
- behavioral posture
- model / thinking / context controls

These are all real, but they are not the same kind of thing.

If we do not separate them explicitly now, implementation will drift into an ambiguous pile where:
- personas implicitly grant permissions
- posture is treated like a trust level
- model tier is mistaken for posture
- context class is mistaken for overall profile identity
- autonomous escalation bypasses workload identity and RBAC reasoning

The right move is to define the stack clearly before wiring more implementation onto it.

## The stack

The runtime stack should be understood as five ordered layers:

```text
Identity -> Authorization -> Persona -> Posture -> Resource Envelope
```

Each layer answers a different question.

## 1. Identity

### Question answered
> Who is this runtime actually acting as?

### Examples
- local operator session
- remote attached controller
- daemon supervisor instance
- future Styrene workload identity via mTLS / signed claims

### Responsibilities
Identity should own:
- principal identity
- issuer / trust source
- session kind
- workload identity claims
- cryptographic or attested proof of identity where applicable

### Non-responsibilities
Identity should **not** directly define:
- posture
- persona
- model tier
- thinking level
- context class

Those are higher-level runtime behavior choices.

## 2. Authorization

### Question answered
> Given this identity, what is it allowed to do?

### Examples
- may attach to a running session
- may adopt a stale workspace lease
- may cancel a cleave child
- may switch to Devastator posture
- may activate a privileged persona
- may perform release-cut actions

### Responsibilities
Authorization should own:
- roles
- capabilities
- tool / action allowlists
- session / workspace / supervisor permission checks
- posture ceilings where required by policy
- control-plane RBAC decisions

### Non-responsibilities
Authorization should **not** define:
- how the harness behaves stylistically
- which expertise lens is active
- the default context width or model tier

It gates those things, but does not replace them.

## 3. Persona

### Question answered
> What expert identity / mind is active?

### Examples
- systems engineer
- benchmark analyst
- release manager
- security auditor
- daemon supervisor persona

### Responsibilities
Persona should own:
- domain framing
- mind store / memory namespace
- voice / communication style defaults
- domain-specific guidance
- preferred posture defaults
- domain-specific tool preferences

### Non-responsibilities
Persona should **not** imply authorization.

A persona may prefer a posture or want to perform an action.
RBAC determines whether it is permitted.

## 4. Posture

### Question answered
> How is the active persona operating right now?

### Posture spectrum
The posture ladder is:

```text
Explorator -> Fabricator -> Architect -> Devastator
```

### Responsibilities
Posture should own the behavioral stance:
- exploration tolerance
- action bias
- escalation behavior
- willingness to spend tokens / time before converging
- answer verbosity and style
- stop / escalate thresholds after sufficient evidence

### Non-responsibilities
Posture should not be used as a trust level or permission tier.
It is a behavior layer.

## 5. Resource Envelope

### Question answered
> With what execution resources is the current identity/persona/posture operating?

### Current subordinate axes
- model tier (`local`, `retribution`, `victory`, `gloriana`)
- thinking level (`off`, `minimal`, `low`, `medium`, `high`)
- context class (`Squad`, `Maniple`, `Clan`, `Legion`)

### Responsibilities
This layer should own the concrete execution envelope:
- selected model tier
- thinking budget
- effective context breadth / working-set class
- future token / wall-time ceilings if posture or policy sets them

### Non-responsibilities
Resource envelope should not define:
- authorization
- persona
- posture identity

It is an execution layer, not a semantic one.

## Relationship rules

### Rule 1 — identity and authorization are trust layers
Identity and authorization together define what the runtime is and what it may do.

### Rule 2 — persona and posture are behavior layers
Persona defines the expertise lens.
Posture defines the operational stance.

### Rule 3 — resource envelope is subordinate execution state
Model tier, thinking level, and context class are downstream controls used to implement the behavior selected by persona and posture.

### Rule 4 — trust may constrain behavior, but behavior must not redefine trust
A posture or persona may request a stronger model or broader control surface.
Authorization decides whether that request is allowed.

## Example runtime composition

```rust
struct RuntimeIdentity {
    principal_id: String,
    issuer: String,
    session_kind: SessionKind,
    claims: IdentityClaims,
}

struct AuthorizationContext {
    roles: Vec<Role>,
    capabilities: Vec<Capability>,
    trust_domain: String,
}

struct PersonaState {
    persona_id: String,
    mind_id: String,
}

enum PosturePreset {
    Explorator,
    Fabricator,
    Architect,
    Devastator,
}

enum PostureMode {
    Fixed(PosturePreset),
    Adaptive { baseline: PosturePreset },
}

struct BehavioralPosture {
    mode: PostureMode,
    effective: PosturePreset,
}

struct ResourceEnvelope {
    model_tier: ModelTier,
    thinking_level: ThinkingLevel,
    context_class: ContextClass,
}

struct OperatingProfile {
    identity: RuntimeIdentity,
    authorization: AuthorizationContext,
    persona: PersonaState,
    posture: BehavioralPosture,
    resources: ResourceEnvelope,
}
```

## Initial mapping to current product surfaces

### `om`
Current conceptual mapping:

```rust
OperatingProfile {
    persona: <active persona>,
    posture: Fixed(Explorator),
    resources: {
        model_tier: Victory or Retribution,
        thinking_level: Minimal,
        context_class: Squad,
    },
    ..
}
```

### full `omegon`
Current conceptual mapping:

```rust
OperatingProfile {
    persona: <active persona>,
    posture: Fixed(Architect),
    resources: {
        model_tier: Victory or Gloriana,
        thinking_level: Medium or High,
        context_class: Maniple / Clan / Legion,
    },
    ..
}
```

### future maximal mode
Potential conceptual mapping:

```rust
OperatingProfile {
    posture: Fixed(Devastator),
    ..
}
```

### future autonomous runtime
Potential conceptual mapping:

```rust
OperatingProfile {
    posture: Adaptive { baseline: Fabricator },
    ..
}
```

## UI / product framing

This stack suggests a layered UX.

### Primary surfaced state
- persona
- posture

### Secondary surfaced state
- model tier
- thinking level
- context class

### Security / diagnostics surface
- identity
- roles / capabilities
- trust domain

A concise operator-facing summary might read:

> Current operating profile: `systems-engineer / Fabricator / Victory / Low / Maniple`

A deeper diagnostic view could additionally include:
- principal identity
- issuer
- session / workload trust domain
- active capabilities

## Surface boundary (implemented)

The runtime model now has a deliberate boundary between **rich local state** and **compatibility/export projections**.

### Canonical internal model
The canonical internal composition is:
- `OperatingProfile` in `core/crates/omegon/src/settings.rs`

This is where the layered stack is modeled directly:
- identity
- authorization
- persona
- posture
- resource envelope

### Canonical local observable surface
The canonical rich local observable surface is:
- `HarnessStatus` in `core/crates/omegon/src/status.rs`

This is the runtime-facing projection used by:
- TUI footer and local runtime views
- local status/bootstrap rendering
- other in-process diagnostics

`HarnessStatus` is allowed to be richer than the external/shared transport contract.

### Compatibility/export surface
The current IPC/web/export surface remains the existing compatibility projection layer:
- `core/crates/omegon/src/ipc/snapshot.rs`
- `core/crates/omegon/src/web/mod.rs`
- `core/crates/omegon/src/web/api.rs`
- shared `omegon_traits::*Snapshot` contracts

These projections are intentionally narrower than `HarnessStatus` today.

### Rule
Do **not** silently widen compatibility/export contracts just because richer local state exists.

If external consumers need the richer model, evolve the shared snapshot contract explicitly as an interface change.

### Current implementation rule
- add new runtime concepts to `OperatingProfile` first
- project them into `HarnessStatus` when they are useful for local observability
- only then decide whether IPC/web/export consumers should receive them

This prevents local architecture work from accidentally becoming an undeclared wire-contract migration.

## Interaction implications

### `/ascend`
`/ascend` should be modeled as a request to increase posture or resource envelope, not as a raw switch to “full mode”.

That request may be constrained by:
- identity claims
- authorization rules
- workspace/session policy
- autonomous runtime posture rules

### Adaptive posture
The future adaptive controller should be able to shift posture based on:
- task breadth
- evidence sufficiency
- open contradiction count
- files touched
- lifecycle scope
- validation state

But those adaptive shifts must still respect authorization ceilings.

## Naming convention guidance

### Canonical axis names
- **Identity**
- **Authorization**
- **Persona**
- **Posture**
- **Resource Envelope**

### Canonical posture ladder
- `Explorator`
- `Fabricator`
- `Architect`
- `Devastator`

### Subordinate resource axes
- model tier: `local`, `retribution`, `victory`, `gloriana`
- thinking level: `off`, `minimal`, `low`, `medium`, `high`
- context class: `Squad`, `Maniple`, `Clan`, `Legion`

### Constraint
Do **not** imply one-to-one ordinal correspondence between these ladders.

For example:
- `Explorator` is not “the same rung” as `Retribution`
- `Architect` is not “the same rung” as `Gloriana`
- `Devastator` is not “the same rung” as `Legion`

A posture preset may *tend* to nudge subordinate controls in a certain direction, but it does not collapse them into the same ladder.

## Decision

Adopt the runtime stack:

```text
Identity -> Authorization -> Persona -> Posture -> Resource Envelope
```

Treat this as the canonical conceptual model for future implementation.

## Next implementation step

Introduce internal types for:
- `PosturePreset`
- `PostureMode`
- `BehavioralPosture`
- `ResourceEnvelope`
- `OperatingProfile`

Then begin migrating current `slim_mode` / full-mode branching into posture presets while keeping identity / authorization concerns explicitly separate.
