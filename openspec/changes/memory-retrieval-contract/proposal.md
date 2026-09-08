# Memory retrieval contract repair

## Intent

Make advertised search capabilities match observable behavior. The inspected
archive tool calls active-only FTS, recall ignores its section argument, and
vector queries identify dimensions but not their embedding space.

## Scope

Typed search intent and filters, safe lexical queries, embedding-space checks,
score descriptions, bounded graph expansion, and consistent domain eligibility.
Retain FTS plus optional vectors and reciprocal-rank fusion. No ANN migration.

## Success criteria

- Historical and current searches return correctly labeled populations.
- Filters apply before candidate truncation in lexical, vector, and graph paths.
- Incompatible embedding spaces are never compared.
- Callers can distinguish no matches, optional-vector degradation, and storage failure.
