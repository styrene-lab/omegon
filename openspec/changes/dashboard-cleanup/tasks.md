# Dashboard cleanup and footer coherence — Tasks

## 1. Refactor the raised footer layout around a pinned metadata block
<!-- specs: dashboard/footer -->

- [x] 1.1 Refactor `extensions/dashboard/footer.ts` so raised mode reserves a fixed bottom metadata block for context/model/thinking, memory summary, and the compact hint
- [x] 1.2 Ensure upper raised-mode sections absorb truncation/compression before the pinned metadata block disappears
- [x] 1.3 Remove redundant raised-mode calls to generic footer-data rendering when that data duplicates dashboard-specific metadata

## 2. Make recovery conditional and tighten OpenSpec formatting
<!-- specs: dashboard/footer -->

- [x] 2.1 Define and implement the actionable-recovery rule in `extensions/dashboard/footer.ts`
- [x] 2.2 Collapse non-actionable recovery to a compact badge or omit it from the expanded section
- [x] 2.3 Tighten `buildOpenSpecLines()` formatting so change rows are compact, visually primary on the name, and lighter on inline separators

## 3. Expose matching dashboard metadata to backend and web UI consumers
<!-- specs: web-ui/dashboard -->

- [x] 3.1 Extend `extensions/web-ui/types.ts` so the dashboard snapshot exposes the operator metadata needed by the cleaned-up footer and a structural recovery-actionability signal
- [x] 3.2 Update `extensions/web-ui/state.ts` so backend snapshots publish the same dashboard-facing metadata inputs used by the TUI cleanup
- [x] 3.3 Add/update web UI/backend state tests proving the metadata is available structurally without parsing rendered footer text

## 4. Add regression coverage for the cleanup behavior
<!-- specs: dashboard/footer, web-ui/dashboard -->

- [x] 4.1 Add/update tests in `extensions/dashboard/footer-raised.test.ts` for pinned bottom metadata and non-duplicated raised footer output
- [x] 4.2 Add/update tests for actionable vs non-actionable recovery rendering
- [x] 4.3 Add/update tests for OpenSpec row spacing/formatting expectations

## 5. Validate the dashboard cleanup slice
<!-- specs: dashboard/footer, web-ui/dashboard -->

- [x] 5.1 Run targeted dashboard and web UI tests covering raised-mode cleanup behavior and metadata parity
- [x] 5.2 Run `npm run typecheck`
