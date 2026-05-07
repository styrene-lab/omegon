+++
id = "5479a0cb-d8f7-4d03-ac68-b3ef23eb7d76"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Controller Tuning Protocol — Deterministic Loop Tuning Under Model Variance

Related:
- [[benchmark-optimization-plan-turn-count-first]]
- [[benchmark-redesign-task-and-eval-spec]]
- [[benchmark-analysis]]

## Why this exists

The controller loop is not a collection of isolated heuristics. It behaves more like a game-engine tick system or scheduler:

- a tiny reset-rule change alters escalation timing
- altered escalation timing changes tool choice
- changed tool choice changes prompt composition and tool history
- changed prompt/history changes model behavior
- changed model behavior feeds back into controller state on the next turn

That means a one-line tweak can create nonlinear benchmark effects.

**Directive:** treat every controller-policy change as a high-leverage systems change. No casual tuning.

## Core principle

Use deterministic process to contain indeterminate models.

We cannot make the model deterministic, but we can make the **tuning workflow** deterministic:
- fixed baselines
- isolated variables
- explicit invariants
- stable prompt surface during controller work
- required before/after benchmark comparisons
- durable documentation of outcome, even for failed experiments

## Non-negotiable tuning rules

### 1. One control-law change per experiment

A controller experiment may change exactly one of:
- reset semantics
- progress classification
- escalation thresholds
- escalation action text/constraints
- slim/full profile threshold divergence

Do **not** combine multiple control-law shifts in one experiment if you want causal attribution.

### 2. Freeze prompt while tuning controller

During controller tuning:
- do not modify slim/full prompt composition
- do not expand lifecycle/operator scaffolding
- do not change static instruction mass

Exception:
- only for very low-capability models or local inference guardrails, and only as an explicitly labeled prompt experiment

Prompt changes are a separate feedback channel and destroy attribution.

### 3. Green tree before release benchmarking

Never benchmark a candidate release from a dirty tree or failing test state.

Required order:
1. implement change
2. run focused tests
3. run full validation
4. commit
5. cut RC
6. verify tag/version/HEAD explicitly
7. only then run release benchmarks

### 4. Verify release state explicitly

`just rc` output is not trusted by itself.

After every RC cut, verify:
- `git status --short` is clean
- `git log --decorate --oneline -N` shows the release commit
- `git tag --list 'vX.Y.Z-rc.*'` includes the new tag
- `cargo run --manifest-path core/Cargo.toml -p omegon -- --version` reports the expected RC

### 5. Keep failed experiments in the record

A failed controller or prompt experiment is still evidence.

Required:
- commit or explicitly revert the experiment
- record the result in docs or memory
- do not silently discard runs that falsify a hypothesis

## Controller invariants

Every controller change should preserve these invariants unless the experiment is explicitly about changing one of them.

### Progress invariants

#### Full reset only on delivery-complete transitions
- successful mutation
- successful commit
- assistant completion

#### Partial reset only on bounded, evidenced progress
- targeted validation
- constraint discovery with supporting evidence and structured cognitive mutation

#### No reset on pseudo-progress
- broad validation with no delivery movement
- repeated repo inspection
- self-reported insight without structured capture

### Understanding-progress invariant

A turn cannot receive understanding-progress credit unless it leaves a structured cognitive artifact.

Acceptable proof-of-work:
- `IntentDocument` delta (constraint, question, failed approach, approach shift)
- lifecycle ambient capture (`<omg:constraint>`, `<omg:question>`, etc.)
- memory fact stored/superseded when the discovery is durable

No cognitive artifact → no understanding-progress credit.

### Anti-gaming invariant

`ConstraintDiscovery` must require both:
1. structured cognitive delta
2. supporting evidence from the same turn window

Allowed evidence sources:
- repo inspection
- validation output
- failed mutation attempt
- other tool output directly tied to the claimed constraint

### Escalation invariant

If the agent repeatedly accumulates understanding progress without delivery progress, controller pressure must resume quickly.

Constraint discovery buys time; it does not buy unlimited analysis.

## Required benchmark baselines

Controller tuning must compare against a known baseline.

Current reference baseline:
- `rc.66` full on `example-shadow-context`
- `rc.66` slim on `example-shadow-context`

At minimum compare:
- wall clock
- total tokens
- turn count
- turn-end reasons
- dominant phases
- drift kinds
- progress nudge reasons

## Required experiment loop

For every controller change:

### Phase A — hypothesis
Record:
- the exact policy change
- the expected effect
- the primary metric that should improve
- the failure mode it is intended to address

Example:
> Change: broad validation no longer fully resets continuation pressure.
> Expected effect: fewer repeated test-only turns after no mutation.
> Primary metric: lower turn count on repair-after-diagnosis tasks.

### Phase B — focused invariants
Run targeted tests that prove the rule behavior directly.

Examples:
- mutation fully resets churn
- broad validation does not reset churn
- targeted validation partially resets churn
- constraint discovery requires evidence + intent delta
- repeated constraint discovery escalates

### Phase C — full validation
Run full validation before any RC cut.

### Phase D — release cut
Cut RC and verify tag/version explicitly.

### Phase E — benchmark comparison
Run the benchmark pair/matrix and compare directly to baseline.

### Phase F — classification
Every experiment is classified as one of:
- **improvement** — metrics improved in the intended direction
- **mixed** — some telemetry improved but benchmark outcome regressed or stayed ambiguous
- **regression** — benchmark outcome worsened
- **invalid** — tree or RC state was not valid, benchmark not trusted

## Benchmark interpretation rules

### Do not celebrate telemetry in isolation

If drift tagging becomes more expressive but wall/tokens/turns worsen, that is **not** a successful controller change.

Telemetry quality and controller quality are separate axes.

### Prefer outcome metrics over elegance

A cleaner controller abstraction that regresses:
- turn count
- token burn
- or wall clock

is still a regression.

### Track both full and slim

The controller is not tuned until both profiles are understood.

A change that helps full and hurts slim is not automatically wrong, but it is incomplete and must be documented as such.

## Suggested metric table for every controller experiment

| Metric | Full | Slim | Delta vs baseline | Interpretation |
|---|---:|---:|---:|---|
| Wall clock |  |  |  |  |
| Total tokens |  |  |  |  |
| Turn count |  |  |  |  |
| Tool continuation turns |  |  |  |  |
| Dominant `orient` turns |  |  |  |  |
| Dominant `act` turns |  |  |  |  |
| Orientation churn count |  |  |  |  |
| Closure stall count |  |  |  |  |
| Validation thrash count |  |  |  |  |
| Constraint discovery count |  |  |  |  |

## Controller-specific design recommendations

### Treat validation as two classes
- **targeted validation** → local progress probe
- **broad validation** → closure attempt, not automatic progress

### Treat understanding progress as a cognitive-state mutation
- session-local via `IntentDocument`
- durable via memory promotion when reusable

### Treat prompt changes as last resort
Prompt modifications should only be considered when:
- controller policy is stable
- benchmark evidence suggests genuine instruction deficiency
- or model capability is so low that controller text must provide hard guardrails

## Memory coupling protocol

Understanding progress should be intimately tied to memory state.

### Minimum bar
Session-local understanding progress requires an `IntentDocument` delta.

### Stronger bar
Durable understanding progress should promote to memory when the discovery is reusable across sessions.

Examples:
- architectural incompatibility
- provider limitation
- recurring benchmark pathology
- stable repo convention or tooling constraint

### Rule
No memory/cognitive mutation, no understanding-progress credit.

## Decision log discipline

For every controller experiment, write down:
- what changed
- what was expected
- what actually happened
- whether it is kept, reverted, or superseded

Do this even for failed experiments.

## Current working stance

- Prompt surface is frozen during controller tuning.
- Controller changes are isolated and benchmarked like engine-tick changes.
- Understanding progress must be evidence-backed and memory/cognition-backed.
- `rc.66` remains the current comparison floor for shadow-context efficiency until superseded by data.
