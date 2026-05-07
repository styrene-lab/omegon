+++
id = "4475c0ab-1a9f-4bfb-83b9-9b0d99e57252"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# 0.6.6 stability step — subprocess boundary hardening and memory search resilience follow-up — Tasks

## 1. extensions/project-memory/factstore.ts (modified)

<!-- specs: memory/search-stability -->
- [x] 1.1 Refine FTS query construction so apostrophes and technical identifier/path-like tokens remain searchable without generating malformed MATCH expressions.
- [x] 1.2 Limit exception swallowing to ignorable query-shape errors and re-throw unrelated operational/storage failures.

## 2. extensions/project-memory/factstore.test.ts (modified)

<!-- specs: memory/search-stability -->
- [x] 2.1 Keep apostrophe regression coverage for searchFacts/searchArchive.
- [x] 2.2 Add regression coverage for path-like and identifier-like technical queries.
- [x] 2.3 Add regression coverage proving non-query operational failures are surfaced rather than silently converted to empty results.

## 3. extensions/lib/omegon-subprocess.ts (modified)

- [x] 3.1 Treat the shared resolver as the canonical internal subprocess entrypoint contract for the audited helper paths from the prior stability fix.

## 4. package.json (modified)

- [x] 4.1 Move the release version bump into the explicit 0.6.6 stability step.

## 5. Verification

<!-- specs: memory/search-stability -->
- [x] 5.1 Run typecheck.
- [x] 5.2 Run targeted project-memory tests covering the new search behavior.
- [x] 5.3 Re-run adversarial diff review after the stability-step fixes land.
