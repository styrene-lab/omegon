# Memory provenance and validity

## Intent

Preserve why a memory is believed, where it applies, and which evidence supports
it. Lifecycle ingestion currently advertises authority and artifact references
that its store branch does not retain.

## Scope

Structured provenance, applicability, temporal validity, lifecycle admission,
supersession evidence, and compatible persistence/transport. Preserve the existing
confirmation policy for inferred lifecycle summaries. No new graph database.

## Success criteria

- Observations, explicit conclusions, and inferred candidates remain distinguishable.
- Lifecycle artifact references and supersession intent survive reopen and transport.
- Current queries enforce applicability; historical queries expose validity.
- Legacy records remain readable without invented evidence or verification dates.
