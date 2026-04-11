---
title: Posture Spectrum — Explorator, Fabricator, Architect, Devastator
status: active
tags: [posture, om, omegon, autonomy, benchmark, runtime]
date: 2026-04-11
---

# Posture Spectrum — Explorator, Fabricator, Architect, Devastator

## Why this exists

Recent benchmark and controller work exposed a latent control axis in the harness.

What looked at first like a simple binary split between `om` and full `omegon` is better understood as a **spectrum of exploratory risk / token burn / breadth tolerance**.

The harness is already being tuned through coordinated bundles of controls:
- thinking budget
- effective context breadth
- tool surface width
- exploration tolerance
- action bias
- escalation bias
- response verbosity
- stop conditions after sufficient evidence exists

That is not a binary flag. It is a posture model.

## Core naming convention

### Spectrum term
Use **posture** as the canonical umbrella term.

Examples:
- current posture: `Explorator`
- escalate posture to `Architect`
- adaptive posture promoted to `Devastator`

### Fixed posture presets
The ladder is:

```text
Explorator -> Fabricator -> Architect -> Devastator
```

These are not quality ranks. They are **behavioral stances**.

## Preset definitions

### 1. Explorator

**Identity:** cheap-first reconnaissance and local hypothesis testing.

**Best for:**
- uncertain local issues
- beginner-friendly first pass
- bounded bug triage
- quick probe before deciding whether a broader harness is warranted

**Primary behaviors:**
- local-first
- one target / one hypothesis / one probe
- smallest reversible action bias
- strong escalate-or-stop bias
- low token burn tolerance

**Current product mapping:**
- successor posture to the current `om` intent

### 2. Fabricator

**Identity:** balanced implementation posture.

**Best for:**
- day-to-day coding
- bounded runtime fixes
- 2–5 file implementation work
- practical operator-guided engineering

**Primary behaviors:**
- implementation-first
- moderate ambiguity tolerance
- enough breadth to connect nearby surfaces
- still cost-aware
- less eager than Explorator to escalate

**Current product mapping:**
- likely future default for many interactive sessions once posture becomes first-class

### 3. Architect

**Identity:** systems-engineering posture.

**Best for:**
- runtime-contract tasks
- cross-surface reasoning
- architectural exploration
- lifecycle/spec-informed work

**Primary behaviors:**
- broader systems framing
- higher exploration tolerance
- explicit tradeoffs and interface reasoning
- less pressure to act immediately when wider context is genuinely needed

**Current product mapping:**
- closest match to the current full `omegon` posture

### 4. Devastator

**Identity:** maximum-force posture.

**Best for:**
- highest-value hard tasks
- expensive rescue operations
- autonomous escalation when thrift is no longer the primary concern
- strongest available model + widest harness affordances

**Primary behaviors:**
- highest token burn tolerance
- widest context and tool surface
- lowest cheap-fail bias
- outcome-first, thrift-second

**Why this rung matters:**
Without Devastator, Architect is forced to mean both:
- broad systems reasoning
- maximum-force operation

Those are related but not identical. The fourth rung separates them cleanly.

## Dynamic runtime policy

The spectrum should also support a dynamic controller mode:

```rust
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
```

### Why `Adaptive`
`Adaptive` describes how the posture changes over time. It is cleaner than overloading `Autonomous` to mean both:
- who is driving
- and how posture shifts

## Underlying controls

Each posture preset should map to a coordinated bundle of controls such as:

```rust
struct PostureControls {
    thinking: ThinkingLevel,
    requested_context_class: ContextClass,
    effective_context_cap_tokens: Option<usize>,

    exploration_tolerance: ExplorationTolerance,
    action_bias: ActionBias,
    escalation_policy: EscalationPolicy,

    response_style: ResponseStyle,
    tool_surface: ToolSurfacePreset,
}
```

### Control families

#### Context posture
- effective working-set cap
- requested breadth class
- compaction aggressiveness
- reserve sizes

#### Tool-surface posture
- minimal local tools only
- implementation-focused local surface
- full harness surface

#### Exploration posture
- max orientation-only turns
- max continuation turns
- max broad search cycles
- post-sufficiency tolerance

#### Action posture
- bias toward smallest reversible patch
- bias toward targeted validation
- willingness to act before broad explanation

#### Escalation posture
- cheap-fail / cheap-escalate
- continue longer before escalation
- adaptive promotion to a stronger posture

#### Response posture
- terse
- concise
- explanatory

## Initial preset mapping

### `om`
Map to:

```rust
Fixed(Explorator)
```

### full `omegon`
Map to:

```rust
Fixed(Architect)
```

### future maximal mode
Map to:

```rust
Fixed(Devastator)
```

### future autonomous runtime
Potentially:

```rust
Adaptive { baseline: Fabricator }
```

or

```rust
Adaptive { baseline: Architect }
```

depending on the desired default autonomy posture.

## Practical implications

### Operator commands
This naming scheme supports a clean future command surface:
- `/posture explorator`
- `/posture fabricator`
- `/posture architect`
- `/posture devastator`
- `/posture adaptive`
- `/ascend`
- `/descend`

### Benchmark interpretation
The benchmark work now makes more sense if interpreted as posture evaluation:
- `Explorator` should dominate bounded local reconnaissance and cheap repair
- `Architect` should dominate richer runtime-contract and systems tasks
- `Devastator` should exist for maximum-force outcome-first runs

### Product framing
This avoids treating `om` and `omegon` as different species.
They become named posture presets over a shared harness.

## Decision

Adopt **posture** as the canonical spectrum term and define the posture ladder as:

```text
Explorator -> Fabricator -> Architect -> Devastator
```

Treat the current `om` / full `omegon` split as an initial pair of presets on that ladder rather than the permanent top-level abstraction.

## Next implementation step

The next engineering step after this naming decision is to introduce a first-class internal posture abstraction (`PosturePreset`, `PostureMode`, and `PostureControls`) and begin migrating current `slim_mode` / full-mode branching onto those preset bundles.
