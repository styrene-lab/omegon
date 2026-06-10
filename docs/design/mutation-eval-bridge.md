+++
id = "3bc44b24-334f-431a-9f9e-664b90a0ec35"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Mutation–Eval Bridge: Impact Attribution Spec

## Status

Draft — decisions made, implementation pending. The wiring between
mutation (runtime observation) and eval (offline measurement) is
architecturally sound. The learning dynamics are **not solved from first
principles** — the values below are our best current reasoning, stated
with enough context that someone with deeper RL expertise can engage,
disagree, and improve without excavating intent from code.

We've chosen transparency over polish. Every decision includes its
reasoning so reviewers can attack the reasoning rather than guessing
at it.

## Terminology: Why "Impact"

In evolutionary computing and reinforcement learning, the standard term
for "how well did this perform?" is "fitness." We deliberately avoid
that word because it overloads to health, exercise, and physical
capability for most people reading this document. The word "impact"
means the same thing here: **did this artifact make the harness better
or worse, and by how much?**

Throughout this document:

- **Impact score** = the computed measure of whether an artifact helped
  (called "fitness function" in RL literature)
- **Impact evaluation** = the process of computing that score from
  observed signals (called "fitness evaluation" in RL literature)
- **Impact data / impact log** = the structured record of every
  evaluation, including all signal values, weights used, and confidence
  changes. This is what would be shared under opt-in telemetry.

If you're coming from an RL background, mentally substitute "fitness"
wherever you see "impact." The math is identical; only the label
changed.

---

## The Core Question

Given a mutation artifact (learned skill or diagnostic), how do we
measure whether it helped?

### Impact Equation

```
I(artifact) = (Σ (signal_i * weight_i) - penalty) / Σ weight_i
```

Where each `signal_i` is a normalized 0.0–1.0 measurement and each
`weight_i` is a tunable coefficient. The `penalty` term captures one
known signal interaction (see Design Decision 2 below). The artifact's
confidence is then updated:

```
new_confidence = clamp(
    old_confidence + (I - neutral_point) * learning_rate,
    floor,
    ceiling
)
```

### Signal Definitions

| Signal | Symbol | Source | How Measured | Seed Weight |
|--------|--------|--------|--------------|-------------|
| Component score delta | `Δ_comp` | eval ScoreCardDiff | Score change on the component(s) this artifact's tags map to, between the eval run before and after the artifact was introduced | 1.0 |
| Burn ratio delta | `Δ_burn` | burn-history.jsonl | Change in session burn ratio between sessions where the artifact loaded vs didn't | 0.8 |
| Recovery recurrence | `R_recur` | mutation trajectory | Did the same recovery pattern (same tool, same RecoveryKind) reappear after the artifact was created? 1.0 = never recurred, 0.0 = recurs every session | 0.6 |
| Turn efficiency | `Δ_turns` | eval ScenarioResult | TurnCount score change on scenarios whose `tests_component` overlaps artifact tags | 0.5 |
| Token efficiency | `Δ_tokens` | eval ScenarioResult | TokenBudget score change on matching scenarios | 0.5 |
| Usage frequency | `U_freq` | provide_context() hit log | How often the artifact was matched and injected. Normalized: 0.0 = never loaded, 1.0 = loaded every matching session | 0.3 |
| Age decay | `D_age` | artifact creation timestamp | Exponential decay from creation date. Not a quality signal — prevents immortal artifacts that were never validated | 0.2 |

### Tuning Parameters

These are the knobs. They live in a single configuration source
(`~/.omegon/mutation/impact.toml`) so they're easy to find, change,
and experiment with.

| Parameter | Symbol | Description | Seed Value | Range |
|-----------|--------|-------------|------------|-------|
| `learning_rate` | `α` | How much a single impact evaluation moves confidence. Deliberately conservative — lightweight passes use noisy data | 0.03 | 0.01–0.2 |
| `neutral_point` | `I₀` | Impact score at which confidence doesn't change. Below this, confidence decays; above, it grows | 0.5 | 0.3–0.7 |
| `confidence_floor` | `c_min` | Minimum confidence before auto-archive | 0.1 | 0.0–0.3 |
| `confidence_ceiling` | `c_max` | Maximum confidence (prevents overfit to one eval run) | 0.95 | 0.8–1.0 |
| `auto_archive_threshold` | `c_archive` | Confidence below which artifact is archived (soft-deleted, recoverable) | 0.15 | 0.05–0.3 |
| `eval_attribution_window` | `t_eval` | Maximum days between eval runs for a diff to be considered attributable | 14 | 7–30 |
| `burn_comparison_sessions` | `n_burn` | Number of recent sessions to average for burn delta comparison | 5 | 3–10 |
| `recurrence_lookback_sessions` | `n_recur` | Sessions to scan for recovery pattern recurrence | 10 | 5–20 |
| `age_half_life_days` | `t_half` | Half-life for age decay signal | 30 | 14–90 |
| `min_eval_runs_for_attribution` | `n_eval_min` | Minimum eval runs with artifact present before impact is computed | 2 | 1–5 |
| `session_cadence` | `n_cadence` | Lightweight impact pass every N sessions (between full eval runs) | 20 | 10–50 |
| `usage_burn_interaction` | `w_interact` | Penalty weight for the "high usage + negative outcome" interaction | 0.5 | 0.0–1.0 |
| `severity_normalizer` | `s_norm` | Token cost at which one recovery equals one recurrence count for escalation scoring | 10000 | 5000–50000 |

---

## Design Decisions

These were open questions. We've answered them with our best reasoning.
Each decision states what we chose, why, what we're uncertain about,
and what would make us reconsider.

### Decision 1: Hand-Set Weights, Not Learned

**Choice:** Signal weights are hand-set in a TOML file, not learned
from data.

**Why:** At the scale we operate (tens to low hundreds of eval runs,
not millions), a statistical optimizer would overfit immediately. The
dataset is too small for machine learning to outperform informed
guessing. More importantly, hand-set weights are debuggable. You can
read the TOML, change one value, run an eval, and see what happened.
A learned model that produces the same weights is opaque.

**What we're uncertain about:** Whether our initial guesses for relative
weight importance are even directionally correct. We assume eval-measured
outcomes (component score delta) are more reliable than runtime
heuristics (burn ratio), but this could be backwards — runtime signals
are continuous while eval runs are periodic snapshots.

**What would make us reconsider:** A dataset of 500+ eval runs with
artifact presence tracking. At that point, even simple regression could
meaningfully estimate weight importance. The system logs every impact
evaluation with all signal values, so the training data accumulates
automatically.

**What we need from others:** If someone with optimization experience
sees a better approach at small-N scales (Bayesian optimization,
multi-armed bandits, Thompson sampling), the architecture supports
swapping in a learned weight provider without changing the signal
collection or confidence update mechanics.

### Decision 2: One Interaction Term, Not Full Non-Linearity

**Choice:** The impact equation is additive with one exception: a
penalty term for the combination of high usage and negative outcome.

```
penalty = max(0, U_freq * (I₀ - Δ_burn)) * usage_burn_interaction
```

This says: if a skill loads frequently AND burn ratio gets worse when
it loads, that's worse than either problem alone. A skill that never
loads and has bad metrics is inert. A skill that loads often and helps
is great. But a skill that loads often and makes things worse is
actively harmful — it's injecting bad advice into context repeatedly.

**Why not full non-linearity:** We can reason about this one
interaction. We can explain it. We can't reason about all pairwise
interactions between 7 signals. Adding interaction terms we don't
understand makes the equation harder to debug without evidence they'd
improve outcomes.

**What we're uncertain about:** Whether other interactions matter. For
instance: does high recovery recurrence combined with low usage
frequency mean the skill isn't being matched when it should be (a
retrieval problem, not a quality problem)? Probably, but we don't yet
have the data to confirm.

**What would make us reconsider:** Evidence (from logged impact
evaluations) that additive scoring systematically misranks artifacts —
e.g., a skill that scores well additively but clearly doesn't help in
practice. That would indicate missing interactions.

### Decision 3: Grace Period for New Artifacts

**Choice:** New artifacts get a grace period of `min_eval_runs_for_
attribution` (seed: 2) eval runs before impact evaluation touches
their confidence. Before that threshold, confidence stays at the
creation default (0.7). Age decay is the safety net — even artifacts
that dodge evaluation decay toward auto-archive over time (30-day
half-life means ~0.175 confidence after 60 days).

**Why:** One eval delta is noise, not signal. A single score change
could be caused by anything — model update, project change, user
behavior shift. Two deltas with the same direction are still noisy but
at least directional. Evaluating on the first run would whipsaw
confidence based on a single data point.

**What we're uncertain about:** Whether 2 runs is enough or too many.
If eval runs happen weekly, a 2-run grace period is 2 weeks of
unevaluated artifacts. If eval runs happen monthly, it's 2 months.
The age decay half-life (30 days) is calibrated to the weekly case;
monthly eval cadence would let artifacts decay significantly before
their first impact evaluation.

**What would make us reconsider:** If we see artifacts accumulating
faster than eval runs can evaluate them, the grace period is too long.
If we see confidence oscillating wildly after the grace period ends,
it's too short (and the real problem is learning rate, not grace
period).

### Decision 4: Session-Count Cadence, Not Time-Based

**Choice:** Lightweight impact passes trigger every N sessions (seed:
20), not on a calendar schedule. This is separate from full eval suite
runs, which remain manual or CI-triggered. The lightweight pass reads
the latest ScoreCard, computes burn deltas from recent history, and
updates confidences. It doesn't run scenarios.

**Why:** Time-based cadence (weekly) punishes inactive users and
under-evaluates active ones. A user who runs 100 sessions a week
generates far more signal than one who runs 3 — both deserve impact
evaluation proportional to their data. Session count scales with actual
usage. 20 sessions gives enough data for the burn comparison window
(5 sessions) to have meaningful averages.

**What we're uncertain about:** Whether lightweight passes (burn-history
only, no eval scenarios) are reliable enough to move confidence. They
have less signal fidelity than full eval runs. We mitigate this by
keeping the learning rate low (0.05) so any single pass makes a small
adjustment.

**What would make us reconsider:** If lightweight passes consistently
disagree with full eval runs (burn metrics say "helpful" but eval
scores say "harmful"), the burn signal is unreliable and lightweight
passes should stop adjusting confidence — just log for analysis.

### Decision 5: Combined Count + Severity for Diagnostic Escalation

**Choice:** Diagnostic escalation uses a combined score:

```
escalation_score = recurrence_count + Σ(token_cost / severity_normalizer)
```

When `escalation_score` exceeds `diagnostic_recurrence_threshold`
(seed: 3), the system generates a candidate eval scenario.

The severity normalizer (seed: 10,000 tokens) is the same value as
the recovery token cost threshold already used in mutation.rs. One
recovery costing 10k tokens adds 1.0 to the escalation score —
equivalent to one recurrence. A catastrophic 30k-token recovery adds
3.0, triggering immediate escalation without waiting for recurrences.

**Why not pure count:** A single 50k-token recovery is more actionable
than three 500-token recoveries. Pure count treats them equally.

**Why not pure severity:** Three cheap-but-frequent recoveries indicate
a systematic problem even if no single instance is expensive. Pure
severity would ignore them.

**Why reuse the existing normalizer:** The mutation feature already
uses 10,000 tokens as a threshold for "this recovery was expensive
enough to warrant analysis." Using the same value for escalation
scoring means the two systems share a consistent definition of
"significant cost." One number to understand, one number to change.

**What we're uncertain about:** Whether 10k tokens is the right
normalizer. It was chosen as the recovery token threshold based on
rough estimates of what "an expensive recovery" looks like. In
practice, recoveries might cluster much lower (1–3k) or much higher
(20–50k), which would make the normalizer too aggressive or too
lenient. We'll know after the first few months of burn-history data.

**What would make us reconsider:** If most diagnostics escalate
immediately (normalizer too low) or never escalate (normalizer too
high), adjust the normalizer. The burn-history JSONL has the raw
token costs for every recovery, so recalibrating is a data query,
not a code change.

---

## Data Flow

### 1. Skill Frontmatter Extension

When mutation creates a skill, tag it with the ComponentMatrix at
creation time. This enables eval attribution: "this skill was created
under these conditions."

Add to learned skill TOML frontmatter:

```toml
[creation_context]
model = "anthropic:claude-sonnet-4-6"
capability_tier = "victory"
thinking_level = "medium"
context_class = "standard"
omegon_version = "0.16.1"
tests_component = ["tools"]      # derived from owning_crate mapping
```

The `tests_component` field uses the same vocabulary as eval scenarios,
enabling direct join between mutation artifacts and eval scores.

### 2. Burn-History Enrichment

Add artifact-presence tracking to `BurnLogEntry`:

```rust
struct BurnLogEntry {
    // ... existing fields ...
    active_learned_skills: Vec<String>,  // names of skills loaded this session
    active_diagnostics: Vec<String>,     // open diagnostic names
}
```

This enables "sessions where skill X was active" vs "sessions where it
wasn't" comparison for burn delta computation.

### 3. Eval Diff Awareness

`ScoreCardDiff` should additionally report:

- Learned skills added/removed between runs (from matrix_changes on
  the `skills` field)
- Burn-history summary between run timestamps (from JSONL scan)
- Recovery pattern frequency between runs (from diagnostic file dates)

This doesn't change the diff computation — it adds context to the diff
output that a human or the impact evaluator can consume.

### 4. Impact Evaluation Triggers

Impact evaluation runs in three modes:

- **Full attribution** — after an eval suite completes (ScoreCard
  stored). Batch evaluation across all artifacts present in the matrix
  using all signals.
- **Lightweight pass** — every `session_cadence` sessions (seed: 20).
  Uses burn-history and recurrence signals only, no eval scenario data.
  Lower-fidelity but continuous.
- **Manual** — via `mutation_stats` tool. Shows current impact signals
  without modifying confidence.

### 5. Confidence Update Pipeline

```
eval run completes (or session cadence reached)
  → load current ScoreCard + previous ScoreCard (if available)
  → compute ScoreCardDiff (full attribution only)
  → for each learned skill:
      → skip if fewer than min_eval_runs_for_attribution
      → compute I(skill) from available signals
      → compute penalty from usage-burn interaction
      → update confidence: c' = clamp(c + (I - I₀) * α, c_min, c_max)
      → if c' < c_archive: archive skill (soft-delete, recoverable)
      → write updated frontmatter
  → for each diagnostic:
      → compute escalation_score = count + Σ(cost / severity_normalizer)
      → if escalation_score >= threshold: generate eval scenario candidate
      → if matching component trend == Improving: mark potentially resolved
```

### 6. Diagnostic-to-Scenario Pipeline

When a diagnostic's escalation score exceeds the threshold, the system
generates a candidate eval scenario TOML:

```toml
# Auto-generated from diagnostic: 2026-04-24-edit-a1b2c3d4.md
[scenario]
name = "edit-uniqueness-recovery"
description = "Agent must handle edit failure on non-unique text"
difficulty = 2
domain = "coding"
tests_component = ["tools"]
generated_from = "diagnostic:2026-04-24-edit-a1b2c3d4"

[input]
prompt = "..."  # Derived from diagnostic reproduction steps

[scoring.recovery_efficiency]
type = "turn-count"
max_turns = 8
ideal_turns = 3
weight = 0.5

[scoring.no_workaround]
type = "tool-allowlist"
allowed = ["read", "edit"]
penalty_per_unexpected = 0.3
weight = 0.5
```

These are written to `~/.omegon/eval-candidates/` for human review,
not auto-added to eval suites. The human decides whether the scenario
is worth formalizing.

---

## Configuration

All tuning parameters live in a single file:

**Path:** `~/.omegon/mutation/impact.toml`

```toml
# Mutation impact evaluation parameters.
#
# "Impact" is what RL literature calls "fitness" — a measure of whether
# a mutation artifact (learned skill or diagnostic) made the harness
# better or worse. We use "impact" because it's clearer to people who
# aren't steeped in evolutionary computing terminology.
#
# These are our best current estimates, not empirically optimized values.
# Every parameter has a stated rationale in docs/design/mutation-eval-bridge.md.
# Change them, run evals, compare ScoreCardDiffs. The architecture is
# designed to make this loop easy.
#
# If you have RL or optimization expertise and see a better approach,
# the signal collection and confidence update mechanics are independent
# of how weights are set — a learned weight provider can slot in without
# changing the rest of the pipeline.

[weights]
component_score_delta = 1.0
burn_ratio_delta = 0.8
recovery_recurrence = 0.6
turn_efficiency = 0.5
token_efficiency = 0.5
usage_frequency = 0.3
age_decay = 0.2
usage_burn_interaction = 0.5

[learning]
learning_rate = 0.03
neutral_point = 0.5

[confidence]
floor = 0.1
ceiling = 0.95
auto_archive_threshold = 0.15

[windows]
eval_attribution_days = 14
burn_comparison_sessions = 5
recurrence_lookback_sessions = 10
age_half_life_days = 30
min_eval_runs_for_attribution = 2
session_cadence = 20

[behavior]
# When false (the default), the mutation system only observes: it logs
# burn-history, detects recovery patterns, and computes impact scores,
# but does not write skill files or diagnostic records. Enable this
# after reviewing the data via `mutation_stats` and confirming the
# signal is real for your workflow.
generate_artifacts = false
# Minimum session turns before recovery detection runs.
min_turns_for_analysis = 8

[escalation]
diagnostic_recurrence_threshold = 3
severity_normalizer = 10000

[telemetry]
# Opt-in to share anonymized impact data with the omegon community
# refinement system. See the "Data Enrichment for Future Federation"
# section in docs/design/mutation-eval-bridge.md for exactly what is
# and isn't shared.
#
# This is OFF by default. No data leaves your machine unless you
# explicitly enable this AND a community refinement endpoint exists.
share_impact_data = false
```

When the file doesn't exist, defaults from the table above apply. The
file is TOML because it's human-editable and the rest of omegon's
configuration uses TOML.

---

## Data Enrichment for Future Federation

All mutation data stays local today. But we structure it so that if a
community refinement system is built in the future, the data is already
in the right shape to aggregate — no retroactive migration needed.

### Why This Matters

The fundamental weakness of single-user impact evaluation is sample
size. One user's 50 eval runs tell you something about one user's
workflow. A thousand users' eval runs could tell you whether a learned
skill is universally helpful, harmful, or context-dependent. The
difference between "this skill helps with Django migrations" and "this
skill helps with Django migrations on Retribution-tier models but hurts
on Gloriana" requires data across diverse environments.

We can't build that system today — it requires consent infrastructure,
privacy review, federation protocol design, and community governance.
But we can make sure the data we're already collecting locally would be
useful if that system existed.

### What "Impact Data" Contains

Every impact evaluation produces a log entry. Here's what's in it and,
just as importantly, what isn't.

**Included — behavioral metrics about the mutation system:**

```rust
struct ImpactLogEntry {
    // ── Identity (anonymizable) ─────────────────────────
    instance_id: String,          // random per-install, NOT user-identifying
    // ── Artifact ────────────────────────────────────────
    artifact_type: String,        // "skill" or "diagnostic"
    artifact_name: String,
    artifact_tags: Vec<String>,
    artifact_age_days: f32,
    // ── Environment (from ComponentMatrix) ──────────────
    model: String,
    capability_tier: String,
    thinking_level: String,
    context_class: String,
    omegon_version: String,
    tool_count: usize,
    extension_count: usize,
    // ── Signal values (all 0.0–1.0) ────────────────────
    component_score_delta: Option<f64>,
    burn_ratio_delta: Option<f64>,
    recovery_recurrence: Option<f64>,
    turn_efficiency: Option<f64>,
    token_efficiency: Option<f64>,
    usage_frequency: f64,
    age_decay: f64,
    // ── Computation ────────────────────────────────────
    penalty: f64,
    impact_score: f64,
    confidence_before: f64,
    confidence_after: f64,
    confidence_delta: f64,
    // ── Weights used (for reproducibility) ─────────────
    weights_snapshot: ImpactWeights,
    // ── Metadata ───────────────────────────────────────
    evaluation_mode: String,      // "full" or "lightweight"
    timestamp: String,
}
```

**Not included — nothing about the user's actual work:**

- No prompt content
- No file paths or project names
- No code snippets
- No usernames, emails, or hardware identifiers
- No session transcripts or conversation content

The `instance_id` deserves specific explanation: it's a random UUID
generated once at install time and stored at `~/.omegon/instance-id`.
It is not derived from any personal information. Its sole purpose is
to let an aggregator distinguish "50 evaluations from one install"
from "1 evaluation each from 50 installs" — a basic statistical
requirement. A user can regenerate it at any time by deleting the file.

### Why the Weights Snapshot Is Included

Each log entry includes the full set of weights that were active when
the evaluation ran. This makes the data self-describing. If different
users run with different weights (because they've tuned their
impact.toml), an aggregator can normalize or filter by weight
configuration without needing to know what version of the config each
user was running.

### Local Storage

Impact evaluation logs are appended to:

**Path:** `~/.omegon/mutation/impact-log.jsonl`

Same append-only pattern as burn-history and upstream-failures. One
JSON line per evaluation. This file grows slowly — one entry per
artifact per impact evaluation trigger.

### Opt-In for Future Sharing

The impact.toml config includes the telemetry section shown above.
Today this flag does nothing — there is no endpoint to send data to.
It exists so that:

1. The consent mechanism is designed before the capability, not after
2. Users can see exactly what would be shared (the log format above)
3. When the system is built, enabling it is a config change, not a
   code change

### What a Future Aggregation System Would Need to Answer

This is explicitly out of scope for implementation. But to validate
that the data format is sufficient, here are the questions it would
need to support:

- "Across all installs running Retribution tier, do skills tagged
  `rust` have positive or negative impact?" (filter by capability_tier
  and tags, aggregate impact_score)
- "What signal weights produce the highest correlation between impact
  score and actual component score improvement?" (regression across
  all entries with non-null component_score_delta)
- "Are there skills that help on one model family but hurt on another?"
  (group by model, compare impact distributions for same artifact_name)
- "What's the right age_half_life? Do old skills actually degrade or
  do they stay useful?" (analyze impact_score vs artifact_age_days)

The ImpactLogEntry format carries enough data to answer all of these
without any additional collection.

---

## What This Does NOT Solve

These are real limitations, not future work. Understanding them
prevents misplaced trust in the system's outputs.

1. **Causal inference.** Score changes after artifact introduction may
   be coincidental. The system detects correlation and surfaces
   confounders (via `matrix_changes`) for human review. It does not
   correct for confounders automatically.

2. **Adversarial robustness.** A malicious eval suite or manipulated
   burn-history could bias impact evaluation. There are no integrity
   checks on signal sources beyond filesystem permissions.

3. **Signal redundancy.** Some signals measure overlapping phenomena
   (burn delta and token efficiency). The additive model double-counts
   this partially. A principled approach would decorrelate signals
   first. We accept the redundancy because it's transparent and
   because removing it requires enough data to estimate correlations
   reliably.

4. **Community aggregation governance.** The data format is ready for
   federation but the governance model is not. Who runs the aggregation
   service? Who reviews the privacy implications? How are contributed
   weights validated before distribution? These are community
   decisions, not engineering ones.

---

## Invitation to Contributors

This system is designed to be improved by people who know more about
learning dynamics than we do. The architecture is deliberately
structured to make that possible:

- **Signal collection** is independent of how weights are set. You can
  swap in learned weights without changing any collection code.
- **Every impact evaluation is logged** with all signal values and the
  resulting confidence change. This is the training dataset for anyone
  who wants to learn weights from data — locally or in aggregate.
- **The TOML config is the single source of truth** for all tuning
  parameters. No magic numbers buried in code.
- **Each design decision documents its uncertainty.** If you think
  Decision 3 is wrong, you can read exactly what we assumed and why,
  and propose a specific alternative.
- **The telemetry opt-in is designed before the system.** When a
  community refinement system is proposed, the consent, data format,
  and privacy boundaries are already defined — not retrofitted.

If you have expertise in reinforcement learning, Bayesian optimization,
multi-armed bandits, causal inference, or federated learning and want
to contribute, the five design decisions above are the leverage points
for local improvements. The data enrichment section above is the
starting point for community-scale work. Everything else is plumbing.

---

## Implementation Sequence

1. **Add `creation_context` to skill frontmatter** — mutation.rs
   generates it from current harness state at skill creation time.

2. **Enrich BurnLogEntry** with `active_learned_skills` — mutation.rs
   records which skills were active when the session ended.

3. **Add impact.toml loader** — new module in mutation feature that
   reads config with defaults, exposes typed `ImpactConfig` struct.
   Includes telemetry opt-in flag (default false, no-op today).

4. **Generate instance-id on first run** — random UUID at
   `~/.omegon/instance-id`, created once, never derived from user
   identity. Used only in impact log entries for future aggregation
   disambiguation.

5. **Add impact evaluation logging** — every evaluation appends an
   `ImpactLogEntry` to `~/.omegon/mutation/impact-log.jsonl` with
   all signal values, weights, environment metadata, and confidence
   delta. This is both the local debugging trail and the future
   community dataset.

6. **Implement impact evaluation** — runs after eval suite completion
   and on session cadence, computes I(artifact) for each artifact,
   updates confidence in frontmatter files.

7. **Extend ScoreCardDiff** with mutation-awareness — report learned
   skill changes and burn-history summary between runs.

8. **Diagnostic-to-scenario generator** — when escalation score
   exceeds threshold, write candidate TOML to eval-candidates
   directory.

Steps 1–5 are foundation. Step 6 is where the seed values get
exercised. Steps 7–8 close the human feedback loop. The telemetry
opt-in exists from step 3 but does nothing until a community
refinement system is built and reviewed.
