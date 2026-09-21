# Evidence-aware memory maintenance

## Intent

Separate truth support from age and attention. Current effective confidence applies
a general decay schedule, while duplicate writes and vault references extend its
influence. Maintenance needs source-aware revalidation and explicit retention policy.

## Scope

Evidence confidence, freshness, salience, validity, retention transitions, source
change detection, reviewable maintenance plans, and auditable outcomes. Preserve
historical retrieval. No automatic deletion from source read failure.

## Success criteria

- Old verified invariants remain usable when still applicable.
- Accesses and references change attention metadata rather than evidence confidence.
- Changed supporting sources trigger bounded revalidation.
- Failed reads preserve evidence and records without falsely refreshing verification.
