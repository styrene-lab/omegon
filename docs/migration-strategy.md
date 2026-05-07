+++
id = "2a2fcff2-7b03-4de2-809a-8bb0c2382206"
kind = "document"
title = "Omegon migration strategy — lean entry, explicit escalation"
status = "implemented"
tags = ["strategy", "migration", "slim-mode", "ux", "autonomy"]
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Omegon migration strategy — lean entry, explicit escalation

This note turns the current friction analysis into a product and implementation strategy.

It is grounded in three current truths:

1. **Claude Code migrants do experience real friction** when they expect Omegon to feel like a lightweight interactive coding loop from the first screen.
2. **Autonomous upstream API-key operators experience real friction** when provider policy, fallback behavior, and execution routing are not expressed consistently across docs, preflight, and runtime behavior.
3. **Omegon should not solve this by pretending to be a different product.** The right move is not to erase the systems harness. The right move is to make the lean path obvious and make escalation into the richer harness explicit.

The strategic principle is therefore:

> **lean entry, explicit escalation**

Operators should be able to start with a low-ceremony coding loop, then opt into memory, lifecycle state, browser surfaces, decomposition, and autonomy controls as the task actually demands them.

---

## Problem statement

Today Omegon is clear about what it is in architecture terms, but less clear in product-flow terms.

The repo already describes two materially different usage shapes:

- **lean interactive path** — `om`, positioned as the fastest interactive path and the comparison profile for mainstream CLI coding agents (`README.md:59-85`, `README.md:180-188`)
- **full systems harness** — default `omegon` with durable memory, design tree, OpenSpec, browser surfaces, and worktree-based decomposition (`README.md:101-153`)

The problem is not that these two shapes exist. The problem is that they are not yet expressed as first-class product modes with clean migration stories.

That produces two classes of failure:

1. **Claude Code-style users land in too much product**
   - too much telemetry
   - too much lifecycle presence
   - too much visible machinery
   - too much ambiguity about the canonical UI
2. **autonomous/headless users land in inconsistent provider policy**
   - docs and code disagree on some automation boundaries
   - supported vs recommended vs automation-safe routing is not expressed with one voice
   - fallback behavior is more conservative than some docs imply

---

## Strategic objective

Create a product shape that is legible from the first minute:

- **Lean interactive** for fast coding work
- **Systems mode** for durable project-state work
- **Autonomous/headless mode** for unattended execution with explicit provider policy

This is not a request for three separate products. It is one harness with three clear operating modes.

---

## Target personas

### 1. Claude Code migrant

**Primary goal**
Get to useful coding work immediately with minimal ceremony.

**Expected interaction style**
- terminal-first
- terse tool feedback
- direct file editing
- minimal setup cognitive load
- no ambient lifecycle instruction unless requested

**Main risk today**
They launch plain `omegon` and interpret intentional harness features as product friction or regressions.

**What they should see instead**
A clear default path into a lean coding loop, with optional escalation into richer features.

---

### 2. Harness-native systems operator

**Primary goal**
Use Omegon as a project operating system: memory, lifecycle, design intent, controlled decomposition, and explicit runtime status.

**Expected interaction style**
- visible telemetry is good
- memory should persist
- design tree and OpenSpec should be close at hand
- decomposition should be normal for larger work

**Main risk today**
Low. This is the product shape Omegon already serves reasonably well.

**What they should see instead**
Current default Omegon, but with cleaner explanation of terminal vs browser surfaces and mode boundaries.

---

### 3. Autonomous/headless operator

**Primary goal**
Run unattended or semi-unattended tasks with predictable provider behavior, explicit preflight, and controlled fallback.

**Expected interaction style**
- policy clarity matters more than UI richness
- provider identity must be explicit
- fallback and auth class must be legible
- failures must explain whether the issue is auth, quota, cooldown, routing, or policy

**Main risk today**
Supported/recommended/automation-safe behavior is not expressed consistently enough across docs, preflight, and runtime.

**What they should see instead**
A clean autonomous contract: what is allowed, what is recommended, what is interactive-only, and what the runtime will do when a provider degrades.

---

## Product modes

### Mode A — Lean interactive

**Purpose**
Fast terminal-native coding loop for operators who want Claude Code-shaped ergonomics.

**Current nearest implementation**
`om`

**Required characteristics**
- terminal-first
- reduced prompt/tool surface
- minimal ambient lifecycle guidance
- compressed footer/telemetry
- direct in-place editing as the default mental model
- tutorial defaults to orientation, not active workflow
- browser surface presented as optional, not central

**Rule**
If an operator arrives wanting “chat + tools + patch files,” they should land here.

---

### Mode B — Systems mode

**Purpose**
Full Omegon harness for durable project-state work.

**Current nearest implementation**
plain `omegon`

**Required characteristics**
- visible runtime telemetry
- memory available and explainable
- lifecycle systems available and discoverable
- decomposition/worktree model surfaced as a normal escalation path
- browser observability surfaces available

**Rule**
This remains the premium default for operators who explicitly want the whole harness.

---

### Mode C — Autonomous/headless mode

**Purpose**
Predictable unattended execution.

**Current nearest implementation**
CLI automation paths plus runtime/provider routing logic

**Required characteristics**
- hard distinction between interactive-only and automation-safe credentials
- explicit preflight output for provider, credential class, fallback policy, and execution posture
- documented supported vs recommended provider matrix
- provider runtime degradation surfaced separately from auth state
- no policy ambiguity between docs and code

**Rule**
If the operator is delegating execution to the harness without close interactive supervision, policy consistency outranks convenience.

---

## Defaulting policy

This is the core product decision.

### First-run/default recommendations

#### Claude Code migrants
Default recommendation:
- **launch `om`**

Not because default Omegon is wrong, but because it is the wrong *starting point* for their expectations.

#### Harness-native operators
Default recommendation:
- **launch `omegon`**

They are opting into the richer product and should get the richer surface.

#### Autonomous/headless operators
Default recommendation:
- **use explicit CLI automation mode with provider preflight**

They do not need slimmer visuals. They need cleaner policy and better execution transparency.

---

## Migration UX policy

### Principle 1: don't hide the harness, stage it

The wrong move would be to suppress Omegon's real strengths in order to imitate Claude Code.

The right move is to stage exposure.

**Stage 1 — start coding**
- lean interactive mode
- minimal telemetry
- orientation-only guidance

**Stage 2 — task gets bigger**
- surface memory and lifecycle as optional assistance
- explain why structured state helps

**Stage 3 — task becomes risky/large**
- surface complexity assessment and worktree decomposition
- explain why isolation exists

**Stage 4 — unattended execution**
- surface autonomous policy, provider routing, and preflight

---

### Principle 2: reduce avoidable surprise

There are several current surprises that should be removed:

1. `--slim` is recommended in README, but not yet framed as a true migration mode
2. telemetry density is too high for lean-first operators
3. lifecycle systems feel ambient even when the operator did not ask for them
4. tutorial intensity is too high for quick-start users
5. `/auspex open` vs `/dash` is still a transition story, not a crisp product story
6. automation-safe provider policy is more nuanced than the current operator-facing story suggests

---

## Implementation priorities

### Priority 0 — clarify the product in docs and first-run messaging

**Why first**
This is the fastest leverage. The architecture already supports much of the desired shape.

**Changes**
1. Introduce explicit mode language in docs:
   - Lean interactive
   - Systems mode
   - Autonomous/headless mode
2. Update migration-facing docs to recommend `omegon --slim` for Claude Code migrants
3. Add a first-run mode-selection explanation or banner
4. Write a short “supported vs recommended vs interactive-only” provider matrix for autonomous usage

**Success criterion**
A new operator can tell which product mode they are entering before they start work.

---

### Priority 1 — make lean interactive a first-class product mode

**Why second**
This is the biggest Claude Code migration win.

**Changes**
1. Treat `--slim` as a named product mode, not just a flag
2. Collapse footer/telemetry aggressively in lean mode
3. Suppress proactive lifecycle/memory nudges in lean mode unless invoked
4. Make orientation tutorial the default tutorial in lean mode
5. Keep direct working-tree editing as the obvious default for simple work

**Potential implementation shape**
- add a named runtime/migration profile rather than relying on a single boolean
- preserve `--slim` as the CLI affordance if desired, but back it with a clearer product concept

**Success criterion**
Claude Code migrants report the product as “immediately usable” rather than “powerful but too much.”

---

### Priority 2 — split onboarding paths

**Why third**
Onboarding is currently too workflow-opinionated for some users.

**Changes**
1. Separate tutorial entrypoints into:
   - Orientation
   - Guided workflow
2. Make orientation the default for lean interactive mode
3. Add up-front copy that explains when a tutorial path may perform real project actions

**Success criterion**
Users who only want command and mode orientation are not forced into a live repo workflow.

---

### Priority 3 — make provider policy and autonomous routing legible

**Why fourth**
This is the biggest autonomous-mode credibility issue.

**Changes**
1. Unify docs and runtime enforcement for Anthropic automation policy
2. Clarify whether execution fallback is provider-local or globally strategic
3. Distinguish:
   - supported
   - recommended
   - automation-safe
   - interactive-only
4. Print autonomous preflight output showing:
   - resolved provider
   - credential class
   - fallback policy
   - any cooldown/degraded state

**Success criterion**
An autonomous operator can predict runtime behavior before the run starts.

---

### Priority 4 — rationalize browser surfaces

**Why fifth**
This is real friction, but not the biggest near-term value.

**Changes**
1. Decide the canonical product story:
   - terminal-primary, browser-optional
   - or browser-primary for some workflows
2. Reduce transitional command duplication once Auspex is mature enough
3. Rewrite docs so `/dash` is clearly either compatibility/debug or a supported primary surface

**Success criterion**
Operators can describe Omegon’s UI model in one sentence.

---

### Priority 5 — preserve worktree decomposition as an escalation path, not ambient pressure

**Why sixth**
This is a strength, but it feels like friction if surfaced too early.

**Changes**
1. Keep decomposition framed as the response to complexity/risk
2. Avoid teaching worktree decomposition as the first thing a lean interactive user sees
3. Add migration copy explaining why Omegon pays this cost: file safety, scope control, deterministic merges

**Success criterion**
Users understand decomposition as a feature for larger work, not ceremony inflicted on small tasks.

---

## Concrete UX changes

### Lean interactive mode should do all of the following

- render a reduced footer by default
- de-emphasize memory counts, auth detail, and posture metadata unless expanded
- hide lifecycle suggestions unless the operator triggers them
- default tutorial to orientation-only
- present browser surfaces as optional
- avoid ambient references to design tree / OpenSpec for simple coding tasks

### Systems mode should do all of the following

- preserve current rich telemetry and lifecycle visibility
- explain why these surfaces exist
- keep memory/lifecycle/decomposition close at hand

### Autonomous/headless mode should do all of the following

- print or expose preflight provider policy clearly
- distinguish auth state from runtime degradation
- avoid warning-only policy for known interactive-only credentials when docs say blocked
- show fallback reasoning before execution

---

## Suggested release order

### Release slice 1 — messaging and migration framing
- docs updates
- first-run mode explanation
- explicit Claude Code migration recommendation to use lean mode
- autonomous provider matrix doc

### Release slice 2 — lean mode cleanup
- compact footer/telemetry
- reduce ambient lifecycle prompts
- orientation tutorial split

### Release slice 3 — autonomy policy consistency
- Anthropic automation enforcement alignment
- fallback semantics alignment
- autonomous preflight transparency

### Release slice 4 — browser surface rationalization
- `/auspex` vs `/dash` cleanup
- revised docs and command guidance

### Release slice 5 — named profiles / richer mode model
- evolve beyond a raw `--slim` boolean if needed
- formalize lean/systems/autonomous product modes in runtime/profile state

---

## Metrics and success criteria

### Claude Code migration metrics

1. **Time to first useful task**
   - how quickly a new operator gets to real coding work
2. **Mode mis-entry rate**
   - how often users who wanted lean coding land in full systems mode first
3. **Tutorial abandonment rate**
   - especially for first-run users
4. **Operator-reported visual complexity**
   - qualitative but important

### Autonomous mode metrics

1. **Preflight clarity**
   - operator can identify provider, auth class, and fallback policy before run
2. **Policy mismatch count**
   - known cases where docs and runtime behavior disagree
3. **Unexpected fallback failures**
   - runs that fail despite another valid provider being available, when operator expected broader fallback
4. **Operator diagnosis latency**
   - how long it takes to understand why automation paused or failed

---

## Non-goals

These are important.

### Non-goal 1: make default Omegon indistinguishable from Claude Code

That would be the wrong product move. Omegon's advantage is that it is a richer systems harness.

### Non-goal 2: hide real runtime/provider state to make the UI feel simpler

The harness should remain honest. The solution is progressive disclosure, not concealment.

### Non-goal 3: broaden autonomous routing by accident

Provider flexibility should come from explicit policy, not from undocumented fallback drift.

---

## Bottom line

Omegon should not choose between being:
- a lean coding loop, and
- a systems engineering harness.

It should be both — but in the right order.

The correct strategy is:

1. **Start operators in the lightest mode that matches their intent**
2. **Make richer harness behavior an explicit escalation**
3. **Make autonomous provider policy predictable and legible**

In short:

> **lean entry, explicit escalation**

That gives Claude Code migrants a believable path into Omegon without amputating the features that make Omegon worth using for larger systems work.
