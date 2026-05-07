+++
id = "cdd6f80a-5f0d-45b4-b550-c976e05053d8"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# facts.jsonl stability — durable transport without runtime churn — Tasks

## 1. Stable transport export in `extensions/project-memory/factstore.ts`

<!-- specs: memory/facts-jsonl-stability -->

- [x] 1.1 Reduce exported fact JSONL records to durable transport fields only, excluding volatile runtime scoring metadata such as `confidence`, `last_reinforced`, `reinforcement_count`, and `decay_rate`
- [x] 1.2 Preserve durable merge/import identity in exported fact records, including fields required for idempotent dedup, supersession tracking, and cross-machine merge=union transport
- [x] 1.3 Keep edge and episode export semantics unchanged unless a test requires a narrowly scoped compatibility adjustment
- [x] 1.4 Keep `importFromJsonl()` backward-compatible with older fact JSONL lines that still include richer legacy metadata

## 2. Regression coverage in `extensions/project-memory/factstore.test.ts`

<!-- specs: memory/facts-jsonl-stability -->

- [x] 2.1 Add a regression test proving reinforcement-only changes do not change exported fact JSONL bytes
- [x] 2.2 Add/adjust a regression test proving durable knowledge changes still change exported JSONL
- [x] 2.3 Add/adjust a regression test proving legacy rich JSONL lines still import successfully under the new stable export contract

## 3. Round-trip and compatibility checks in `extensions/project-memory/vectors-episodes.test.ts`

<!-- specs: memory/facts-jsonl-stability -->

- [x] 3.1 Update JSONL round-trip expectations only where necessary to match the stable transport contract
- [x] 3.2 Verify episode/edge round-trip behavior is still preserved after fact-line export trimming

## 4. Design + operator-facing documentation

<!-- specs: memory/facts-jsonl-stability -->

- [x] 4.1 Update `docs/facts-jsonl-stability.md` to record the accepted durable field set and the rationale for excluding volatile runtime metadata

## 5. Validation

<!-- specs: memory/facts-jsonl-stability -->

- [x] 5.1 Run targeted project-memory tests covering fact JSONL export/import stability
- [x] 5.2 Run `npm run typecheck`
- [x] 5.3 Run `/assess spec facts-jsonl-stability`
