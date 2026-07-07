+++
title = "Harness Loop OODA & Churn Guidance — Friction Audit"
status = "exploring"
+++

# Harness Loop OODA & Churn Guidance — Friction Audit

Branch: `deep-dive-agent-harness-ooda`. Audit of the internal nudge/OODA/churn
control stack. Motivating symptom (operator-reported, multiple instances): the
agent truncates legitimate work and reports *"I didn't do anything except
research because the harness told me to."* The guidance layer is acting as
brakepads on legitimate long-form investigation.

## System inventory (evidence-grounded)

All line references at `main` tip `5cc1c2c6`.

### Layer 1 — Tool capability taxonomy
`behavior.rs:16-127`. `ToolCapabilityCatalog` maps tool name → capability
labels (`Orientation`, `RepoInspection` broad/targeted, `Mutation`,
`Validation`, `StateChanging`, `ProgressBoundary`). Every downstream
classifier keys off these labels.

### Layer 2 — OODA phase classifier
`classify_turn_phase` (`behavior.rs:128-185`). Emits Observe/Orient/Act only.
`OodaPhase::Decide` exists in `omegon-traits` and renders in the statusline
(`tui/statusline.rs:327`) but is **never emitted** — display fiction.

### Layer 3 — Drift classification
`classify_drift_kind` (`behavior.rs:187+`). Four kinds:

| Drift | Trigger | Nudge reason |
|---|---|---|
| OrientationChurn | no files modified, files read, all-repo-inspection turn, turn ≥ 4, any broad inspection, ≤ 1 targeted | AntiOrientation |
| OrientationChurn (early) | nothing read/modified, turn ≥ 3, all broad orientation | AntiOrientation |
| RepeatedActionFailure | ≥ 2 failing mutations, same tool + path | ActionRecovery |
| ValidationThrash | ≥ 2 validation calls, no files modified, not targeted | ValidationPressure |
| ClosureStall | files modified but turn all-inspection with broad calls | ClosurePressure |

### Layer 4 — ControllerState streaks
`behavior.rs:395+`. Per-drift streaks: increment on match, **halve** on
mismatch (never hard-clear except on Mutation/Commit/Completion).
TargetedValidation/ConstraintDiscovery halve orientation/continuation, zero
failure/thrash.

### Layer 5 — Evidence sufficiency
`assess_evidence` (`behavior.rs:632`). `files_read ≤ 2` + targeted-only reads
of known paths ⇒ local evidence "Actionable". Feeds
`local_evidence_sufficient_streak` / `evidence_sufficient_streak`.

### Layer 6 — Behavioral tiers & thresholds
`behavioral_tier` (`behavior.rs`): Frontier/Max ⇒ Standard, Mid/Leaf ⇒
Constrained. `continuation_pressure_tier` (`behavior.rs:736-796`) thresholds
(tier1/2/3 = turns of continuation before pressure):

| Mode | Constrained | Standard |
|---|---|---|
| om_local_first_lock (Slim UI + local evidence + read-not-modified) | 2/3/5 | 4/6/8 |
| evidence_sufficient | 3/4/6 | 6/8/10 |
| slim execution bias | 4/6/8 | 8/12/16 |
| default | 3/5/7 | 12/16/20 |

### Layer 7 — Nudge injection cascade
`loop.rs:1479-1550`, priority order (first match wins, one per turn):
1. `first_turn_execution_bias` (`is_first_turn_orientation_churn`)
2. `om_local_first_lock` → "You have enough context. Produce the requested output…"
3. `evidence_sufficiency` → forced convergence
4. `execution_pressure` (`should_inject_execution_pressure`, turn ≥ 3-6 by tier)
5. `continuation_pressure_tier_{1,2,3}`

Plus text-only-turn injectors (`loop.rs:942-1284`):
- meta-recovery (max 2) — `is_pathological_meta_response`
- commit hygiene (once/session, near budget or completion language)
- plan reconciliation (fingerprinted, repeatable)
- skill phase completion (once/session)
- dead-mouse auto-continue (max 3; guarded by `should_continue_text_only_turn`
  — automation level, question detection, completion/blocked detection,
  `looks_like_continuation_request`)

### Layer 8 — StuckDetector
`loop.rs:4257+`. Sliding window (10): same-target re-reads ≥ 5 without
mutation/validation; same tool+args ≥ 3; ≥ 3 consecutive same-tool errors.
Path-normalized hashing collapses offset/limit variations. Skips detection if
any mutation/validation appears in window.

Every injected nudge consumes a turn (`TurnEndReason::ProgressNudge`) and
recomputes/re-emits context composition.

## Friction points

**F1 — Research work is structurally classified as pathology.**
`files_modified.is_empty()` is the core stall predicate in OrientationChurn,
execution pressure, `has_local_target_hypothesis`, and evidence sufficiency.
A directed deep-dive/audit *never* modifies files, so from turn ~4 the
classifiers read sustained legitimate investigation as drift. The
`om_local_first_lock` fires precisely on "read files but modified none" —
i.e., on the definition of research.

**F2 — Nudge wording says "you have enough context" when the harness cannot
know that.** `evidence_sufficiency_message`, `om_local_first_message`, and
execution-pressure all assert sufficiency from a 2-files-read heuristic
(`assess_evidence`: `local_target_count <= 2`). For a deep dive spanning an
8k-line `loop.rs` and 1k-line `behavior.rs`, 2 files ≠ enough. The model
obeys the assertion and truncates — producing exactly the reported "the
harness told me to stop" behavior. Verified live: this audit session received
`om_local_first_message` after 2 read-turns and answered before finishing the
planned reads.

**F3 — No task-intent input.** `IntentDocument.lifecycle_phase` exists but
`classify_drift_kind` / pressure tiers consult only files_read/modified and
tool labels. "Operator asked for analysis" is indistinguishable from "agent
is lost." The dead-mouse path *does* parse operator prompts
(`user_asked_question` heuristics) — the drift path does not.

**F4 — Absolute turn thresholds.** `turn >= 4` fires identically for a typo
fix and a multi-thousand-line subsystem audit. No scaling by evidence
accumulation rate, file sizes, or directive complexity.

**F5 — Nudges consume budget.** Each nudge burns a turn against `max_turns`
plus prompt mass. Worst case per session: 2 meta + 3 dead-mouse + 1 commit +
N plan-reconciliation + 1 skill + per-turn drift nudges.

**F6 — evidence_sufficient "streak" is a misnomer.** `local <= 2 files read`
⇒ Actionable; a *broad* investigation (many files) yields
`EvidenceSufficiency::None` — paradoxically, reading *more* protects you
from convergence pressure while a tight targeted read triggers it fastest.
The incentive gradient points the wrong way for focused work.

**F7 — OODA Decide phase is dead.** Emitted nowhere; statusline renders a
4-phase model the classifier can't produce. Cosmetic debt, but it also means
no "deliberation" state exists between Observe and forced Act — the control
model literally has no vocabulary for "still deciding."

**F8 — Priority cascade can double-tap.** A dead-mouse nudge on a text turn
followed by an om_local_first nudge on the next tool turn gives the model two
contradictory instructions within two turns ("take action now" then "stop
searching, answer now"). Verified live in this session.

## Fix directions (not yet decided)

1. **Task-mode signal.** Infer or accept operator declaration of
   research/analysis mode; suppress OrientationChurn + execution pressure +
   om_local_first_lock while active. Keep RepeatedActionFailure, StuckDetector,
   ValidationThrash (genuine pathology regardless of mode).
2. **Honest wording.** Replace "You have enough context" with a question the
   model can veto: "If you have enough evidence, answer now; if not, state
   what you still need and continue." Assertion → checkpoint.
3. **Evidence-relative thresholds.** Scale tier thresholds by
   novel-information rate (new files/paths per turn) instead of absolute turn
   counts; a session still discovering new targets is not churning.
4. **Nudge budget accounting.** Cap total injected nudges per session and/or
   exempt nudge turns from `max_turns`.
5. **Fix or remove Decide.** Either emit it (e.g., text-only turn with
   reasoning after evidence gathering) or drop it from the statusline.

## Open questions

- [ ] [assumption] The model treats nudge text as scope directive rather than
  hint — inferred from operator reports + one live observation; needs session
  log corpus confirmation.
- [ ] Where does `dominant_phase` get computed per turn, and is phase
  *history* consulted anywhere? (Believed no — unverified.)
- [ ] Does `should_nudge_plan_reconciliation` loop indefinitely on a
  fingerprint-stable plan? (`plan_reconciliation_nudges` counts but cap not
  yet located.)
- [ ] What writes `intent.files_read` — do `codebase_search`/`view` count, or
  only `read`? Determines how often research undercounts as "no evidence."
