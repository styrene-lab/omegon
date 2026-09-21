# Memory evaluation corpus

## Intent

Establish reproducible evidence that memory improves coding-harness behavior.
Existing storage and lifecycle tests do not establish extraction fidelity,
retrieval quality, stale-memory avoidance, or reuse of successful experience.

## Scope

Add synthetic domain fixtures, multi-session replay cases, memory ablations, and
structured quality/cost reporting. Live-model evaluation is a separate opt-in
tier. This change does not replace storage or tune retrieval policy.

## Success criteria

- Deterministic evaluation runs without credentials or network access.
- Gold answers and future evidence never enter ingestion or query inputs.
- Reports distinguish formation, retrieval, selection, and task failures.
- No-memory, file-search, current-memory, and candidate-policy runs use the same
  workload, reader configuration, and declared resource budgets.
