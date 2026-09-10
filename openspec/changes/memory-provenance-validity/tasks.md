## 1. Evidence and applicability vocabulary
<!-- specs: memory/provenance -->

- [x] Introduce the minimal typed episode formation vocabulary: source frontier, attributed excerpts, observed tool outcomes, pending candidates, and extraction state.
- [x] Preserve the source evidence across atomic completion; an assistant report cannot carry a tool outcome and generated candidates are not active facts.
- [ ] Resolve canonical workspace/revision descriptors and evidence retention behavior before freezing schema fields.
- [ ] RED: Add observed-versus-inferred, imported-authority, platform applicability, historical validity, and unavailable-source tests.
- [ ] GREEN: Implement domain evidence/applicability types and retrieval inspection using host-owned source identities.
- [ ] REFACTOR: Keep authority interpretation out of renderers and transport adapters.

## 2. Lifecycle admission and atomic corrections
<!-- specs: memory/lifecycle -->

- [ ] RED: Exercise artifact retention, explicit constraints, archived specs, pending inferred summaries, replay, and version-conflict rollback via lifecycle ingestion.
- [ ] GREEN: Persist authority and references, enforce the existing confirmation policy, and apply supersedes through atomic mutation contracts.
- [ ] REFACTOR: Share admission semantics with ordinary domain callers without duplicating the lifecycle engine.

## 3. Persistence and verification
<!-- specs: memory/provenance, memory/lifecycle -->

- [x] Add nullable episode formation storage in schema v9, supported v5–v8 migration, legacy unknown preservation, and JSONL/reopen fixtures.
- [x] Verify migration rollback and atomic completion of episode metadata, search index, vector invalidation, and receipts.
- [x] Complete the scoped Wave 3 landing gate linked from ../memory-evidence-capture/verification-wave-3.md.
- [ ] RED: Add supported legacy DB and JSONL fixtures plus failed-migration, reopen, and vault round-trip cases.
- [ ] GREEN: Implement migration and schema-contract updates preserving legacy unknowns and operational metadata.
- [ ] Verify vault idempotency/path boundaries and both backends; record scenario mappings and red/green outcomes.
- [ ] Run applicable landing gates from ../../memory-modernization.md and validate this change.

## 4. Wave 5 transport prerequisite
<!-- specs: memory/provenance -->

- [x] RED/GREEN: Preserve all four fact statuses and operational metadata across SQLite/in-memory transport; retain unknown source labels.
- [x] Verify legacy-update preservation, modern version-controlled replacement, idempotency, historical edge endpoints, reopen, and rejected-batch receipt rollback.
- [x] Record the transport landing gate and same-executor adversarial review in verification-wave-5-transport.md.

## 5. Wave 5 inferred lifecycle candidates
<!-- specs: memory/lifecycle, memory/provenance -->

- [x] RED/GREEN: Reproduce active ingestion of inferred lifecycle summaries at the live tool boundary; retain them as pending candidates.
- [x] Add typed declared-artifact attribution and proposed supersession, preserving operation replay without reinforcing identical active facts.
- [x] Verify schema-v10 migration, reopen, transport, index/context/vault exclusion, and failed-write rollback; record final gates in verification-wave-5-candidates.md.
- [x] Complete candidate confirmation with trusted admission and atomic correction handling; explicit artifact admission is covered below.

## 6. Wave 5 explicit artifact admission
<!-- specs: memory/lifecycle, memory/provenance -->

- [x] RED/GREEN: Reject unsupported explicit assertions at the live tool boundary and validate structured design/specification conclusions with existing parsers.
- [x] Retain portable artifact/statement hashes and references without inferring execution evidence; reject proposal paths, open questions, symlinks, and oversized artifacts.
- [x] Apply same-mind, version-checked corrections atomically and verify receipt replay, rollback, attribution-aware reuse, reopen, and JSONL transport.
- [x] Record final focused checks, landing gates, and same-executor review in verification-wave-5-explicit.md.

## 7. Wave 5 operator confirmation
<!-- specs: memory/lifecycle, memory/provenance -->

- [x] RED/GREEN: Add snapshot-bound confirmation with recorded operator context, stale-version rejection, atomic correction, receipt replay, reopen, and transport tests.
- [x] Route public review requests through TUI/ACP permissions and keep the commit invocation internal; reject agent approval flags and non-internal dispatch.
- [x] Complete schema-v12, corruption/vault-boundary, frontend, and final landing gates; record acceptance in verification-wave-5-confirmation.md.

## 8. Wave 5 provenance inspection
<!-- specs: memory/provenance -->

- [x] RED/GREEN: Add shared status-neutral inspection, with explicit legacy/inferred/confirmed/artifact attribution and bounded previews.
- [x] Verify missing, changed, readable-unverified, and unsupported artifact references without altering durable state.
- [x] Complete cross-backend/provider checks and host/landing gates in verification-wave-5-inspection.md.
