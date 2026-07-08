# A3 evidence ledger — Tasks

## 1. Ledger data model
<!-- specs: harness-guidance/evidence-ledger -->

- [x] 1.1 Add serializable evidence ledger state to `IntentDocument`.
- [x] 1.2 Track seen paths and per-turn novelty/revisit/mutation-validation summaries.
- [x] 1.3 Add unit coverage for novel reads, revisits, and boundary interruption.

## 2. Evidence assessment integration
<!-- specs: harness-guidance/evidence-ledger -->

- [x] 2.1 Feed ledger state from normalized observations.
- [x] 2.2 Replace `files_read <= 2` actionability with low-novelty target-hypothesis actionability.
- [x] 2.3 Add regressions for first targeted read vs repeated low-novelty revisits.

## 3. Validation and release memory
<!-- specs: harness-guidance/evidence-ledger -->

- [x] 3.1 Run focused behavior/conversation tests.
- [x] 3.2 Run `cargo test -p omegon --locked`.
- [x] 3.3 Update `CHANGELOG.md`.
