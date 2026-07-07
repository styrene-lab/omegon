+++
title = "Harness Loop Guidance — Design Affordances"
status = "exploring"
+++

# Harness Loop Guidance — Design Affordances

Companion to `harness-loop-ooda-friction-audit.md`. Defines the architectural
primitives required to move the guidance system from "brakepads on legitimate
work" to a controller that can distinguish healthy investigation from churn.

New evidence since the audit (`conversation.rs:349-380`):
`IntentDocument::update_from_tools` populates `files_read` from **hardcoded
tool names** (`"read" | "understand"`) and `files_modified` from
(`"change" | "write" | "edit"`). Reads via `bash` (grep/sed/cat),
`codebase_search`, and `view` count as *nothing*. The controller's primary
sensor is blind to most real research activity, and it bypasses the
capability catalog that every other layer uses. This makes sensor
unification the foundational affordance — classification fixes built on the
current sensor inherit its blindness.

## Affordance map

Dependency order: A2 → (A1, A3) → (A4, A5) → (A6, A7, A8).

### A2 — Unified observation layer (foundational)

**Problem it removes:** triple bookkeeping + sensor blindness. Today
`IntentDocument`, `ControllerState`/drift, and `StuckDetector` each parse raw
tool calls independently, with different taxonomies (string match vs.
capability catalog vs. path-normalized hashes).

**Affordance:** a single `ObservationNormalizer` that maps every
`(ToolCall, ToolResultEntry)` pair → semantic observation events:

```
ObservationEvent =
  | FileRead { path, source_tool, novel: bool }
  | FileMutated { path }
  | SearchPerformed { scope: Broad | Targeted, novel_hits: usize }
  | ValidationRun { scope: Broad | Targeted, passed: bool }
  | ProgressBoundary { kind: Commit | Completion }
  | ExternalAction { .. }
```

- Derives classification from `ToolCapabilityCatalog`, never name strings.
- Classifies `bash` commands into read/search/mutation/validation via a
  conservative arg classifier (grep/rg/sed -n/cat/ls → read;
  cargo test/check → validation; git commit → boundary). Unknown → opaque,
  not "no evidence".
- `IntentDocument`, drift classification, evidence assessment, and
  StuckDetector all consume the event stream. One sensor, one truth.

**Cost:** bash arg classification is heuristic and will misclassify edge
cases. Acceptable: today's baseline is 100% misclassification for bash reads.

### A1 — Task-mode contract (intent channel)

**Problem it removes:** F1/F3 — the controller cannot distinguish "operator
asked for analysis" from "agent is lost".

**Affordance:** a first-class `TaskMode` on the session/turn:

```
TaskMode = Implementation | Research | QA | Maintenance
```

Sources, in precedence order:
1. Operator declaration (`/mode research`, or per-prompt marker).
2. Directive classification at turn 0 (the dead-mouse path already has
   `user_asked_question` heuristics — promote and share them).
3. Agent self-declaration via the `plan` tool (a plan of read-steps ⇒
   Research), revisable and audited.

Consumption: drift rules and pressure tiers become **mode-conditional policy
rows** instead of hardcoded predicates. In Research mode:
- OrientationChurn, execution pressure, `om_local_first_lock`,
  evidence-sufficiency convergence: suppressed or thresholds ×3.
- RepeatedActionFailure, StuckDetector, ValidationThrash: unchanged —
  genuine pathology in any mode.
- Deliverable pressure changes shape: "produce findings" not "produce edits".

### A3 — Evidence ledger (discovery-rate model)

**Problem it removes:** F4/F6 — absolute turn thresholds and the perverse
`files_read <= 2 ⇒ Actionable` gradient that pressures targeted work fastest
while broad wandering escapes.

**Affordance:** replace scalar file counters with a per-session ledger
tracking novelty:

- `novelty_rate`: fraction of this turn's observations touching paths/symbols
  not previously seen (directly computable from `FileRead.novel` /
  `SearchPerformed.novel_hits`).
- `revisit_rate`: re-reads of already-seen targets without interleaved
  mutation/validation (StuckDetector's pattern 1, generalized).

Controller semantics: **high novelty = healthy investigation regardless of
turn count; falling novelty + no output = churn.** Convergence pressure keys
on the derivative, not the odometer. `assess_evidence` becomes "novelty has
decayed below threshold for K turns AND a target hypothesis exists" instead
of "≤ 2 files read".

### A4 — Vetoable checkpoints (nudge protocol)

**Problem it removes:** F2 — nudges assert facts the harness cannot know
("You have enough context") and models obey them as scope directives.

**Affordance:** nudges become structured checkpoints with an explicit
continue path:

- Wording contract: state the *observation* (turns without output, novelty
  decay), then offer the fork: "Answer now if you have enough evidence;
  otherwise state in one line what you still need, and continue."
- A justified continuation is recognized (cheap regex/marker on next
  assistant text) and **suppresses that nudge class for K turns** — the
  model can push back without being re-nudged every turn.
- Escalation only when continuations repeat without novelty (ties into A3).

Never assert epistemic states. The harness observes behavior; the model owns
the sufficiency judgment; the operator owns the override.

### A5 — Nudge arbiter (single actuator with budget)

**Problem it removes:** F5/F8 — scattered injectors (cascade at
`loop.rs:1479`, five text-only injectors, StuckDetector) with independent
counters, no contradiction prevention, and turn-budget consumption.

**Affordance:** one `NudgeArbiter` owning all injection:

- Every candidate nudge is a `(class, priority, message)` submitted to the
  arbiter; at most one fires per turn (already true for the cascade; not
  true across cascade + text-only paths).
- **Consistency memory:** the arbiter records the last directive issued and
  refuses to issue a semantically contradictory one within K turns (e.g.,
  dead-mouse "take action now" followed by local-first "stop searching").
- Per-class caps and a session-wide nudge budget in one place (today:
  scattered `dead_mouse_nudges`, `meta_recovery_nudges`,
  `commit_nudged`, `plan_reconciliation_nudges`, `skill_completion_nudged`,
  `consecutive_warnings`).
- Nudge turns accounted separately from `max_turns` (or the budget is
  extended by nudges injected) so guidance doesn't eat the work budget.
- Single emission point for `BusEvent::NudgeInjected` with class + outcome
  slot (feeds A7).

### A6 — Phase history (make OODA load-bearing or drop Decide)

**Problem it removes:** F7 — per-turn phase snapshots with no rhythm model;
Decide rendered but never emitted.

**Affordance:** a phase-history ring on the controller:

- Healthy rhythms (Observe→Orient oscillation while novelty is high;
  Act→Observe verification loops) are recognized and *exempt* from
  continuation pressure.
- `Decide` gets real semantics: emitted when a turn produces
  reasoning/plan/decision text after evidence-gathering (text +
  plan-tool calls, no mutation). This is the missing vocabulary for "the
  model is deliberating" — currently classified as dead-mouse territory.
- If this is not adopted, remove Decide from the statusline; a control
  surface must not display states the controller cannot produce.

### A7 — Controller telemetry & outcome tracking

**Problem it removes:** the tuning loop is blind. We cannot currently answer
"do models comply with, ignore, or over-obey nudges?" except by operator
anecdote (the motivating symptom of this whole effort).

**Affordance:**
- Extend `NudgeInjected` with an outcome field resolved on the following
  turn: `Complied | Truncated | JustifiedContinue | Ignored`.
  `Truncated` = produced final answer with markedly lower output than the
  session's evidence trajectory implied (heuristic, flagged not judged).
- Per-session nudge report in the session log; corpus-level offline
  evaluation script over session logs to validate the audit's standing
  [assumption] that nudge text acts as a scope directive.
- Threshold changes (A3/A8) get evaluated against the corpus before and
  after. The controller becomes empirically tunable instead of vibes-tuned.

### A8 — Guidance policy as configuration

**Problem it removes:** thresholds are hardcoded tuples inside match arms
(`continuation_pressure_tier`); every tuning experiment is a recompile.

**Affordance:** a `GuidancePolicy` struct — per `(BehavioralTier, TaskMode)`
row: pressure tiers, drift turn-thresholds, novelty decay constants, nudge
class caps, checkpoint cooldowns. Loaded from settings with a Pkl schema
(consistent with the existing `pkl/` config surface), defaulting to current
values. Operators who find the leash tight can loosen it without a fork;
experiments become config diffs.

## What deliberately stays

- StuckDetector's three patterns — the mutation-clears-path and
  path-normalized hashing design is sound; it just moves onto the A2 event
  stream.
- RepeatedActionFailure and dead-mouse detection (with A5 arbitration) —
  these catch real failure modes across all task modes.
- The anti-meta-spiral messaging ("do not apologize / self-criticize") —
  orthogonal to the friction and demonstrably needed for some models.
- Commit hygiene and plan reconciliation nudges — correct as-is, capped,
  fire on real closure obligations.

## Sequencing proposal

1. **A2** — sensor unification (largest correctness fix, enables everything).
2. **A1 + A3** — mode contract + evidence ledger (kills F1/F3/F4/F6).
3. **A4 + A5** — checkpoint protocol + arbiter (kills F2/F5/F8).
4. **A7** — telemetry (validates the above empirically).
5. **A6 + A8** — phase history and policy config (hardening).

Each stage is independently shippable and independently testable against the
existing `loop.rs` test corpus (~60 tests already cover
`continuation_pressure_tier` / `should_inject_execution_pressure` behavior
and will pin regressions).

## Open questions

- [ ] [assumption] Bash arg classification can reach useful precision with a
  small conservative ruleset — needs a sample of real session bash commands.
- [ ] Should `TaskMode` be mutually exclusive or a per-turn blend? (A deep
  dive often ends in a patch; mode transition semantics needed.)
- [ ] Does justified-continue recognition (A4) need a structured tool/marker,
  or is text-pattern recognition reliable enough across providers?
- [ ] Where does `dominant_phase` get computed today, and can the A6 ring
  reuse it directly? (Still unlocated outside TurnEnd emission sites.)
- [ ] Nudge-turn budget exemption: exempt entirely, or cap exemptions to
  avoid unbounded sessions under `Autonomous` automation?
