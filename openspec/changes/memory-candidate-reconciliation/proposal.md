# Memory candidate reconciliation

## Intent

Replace append-or-exact-hash behavior with evidence-aware admission. Repeated
paraphrases, explicit corrections, refinements, and unresolved contradictions
require different durable outcomes.

## Scope

A bounded candidate reconciliation service with new/equivalent/refinement/
correction/conflict decisions, source-aware evidence accounting, and atomic
versioned admission. Foreground writes, lifecycle ingestion, and background
extraction use the same domain rules with explicit admission intent.

## Success criteria

- Equivalent claims do not multiply active facts.
- Explicit corrections preserve historical evidence and supersession.
- Unresolved contradictions remain inspectable without silently choosing the newest.
- Retries and repeated evidence do not masquerade as independent confirmation.
