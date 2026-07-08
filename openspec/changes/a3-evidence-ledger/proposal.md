# A3 evidence ledger

## Intent

Replace scalar evidence-convergence thresholds with a per-session discovery-rate ledger so guidance distinguishes healthy investigation from repeated low-novelty churn.

## Scope

- Track per-turn observation totals, novel file reads, revisits, searches, and mutation/validation boundaries.
- Feed the ledger from normalized observation events in `IntentDocument::update_from_tools`.
- Use the ledger in evidence assessment so targeted reads become actionable only after novelty decays for repeated turns, not merely because few files were read.
- Preserve mutation/validation/failure-driven actionability.

## Out of scope

- A4 vetoable checkpoint wording.
- A5 nudge arbiter consolidation.
- Symbol-level novelty and search hit counts beyond the minimal A3 path/search ledger.

## Success criteria

- Novel file discovery keeps evidence below forced-convergence actionability.
- Repeated revisits to known targets without mutation/validation make a known target hypothesis actionable.
- Mutation and validation boundaries reset/interrupt revisit decay.
