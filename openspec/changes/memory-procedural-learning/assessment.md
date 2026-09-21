# Wave 8 drift assessment

## Evidence boundary and authority

Assessed against `main` commit `7b2da402` after Wave 5 acceptance. This document
describes current evidence and planned Wave 8 work, not completed implementation.
The root `AGENTS.md` and actual crate ownership contracts govern implementation.
The delta spec defines planned behavior. Source and archived results establish
what exists now; older comments do not override executable admission behavior.

This change is prepared for parent isolation on
`feat/memory-wave8-procedural-learning` directly from that baseline. Branch,
upstream issue, and pull-request operations belong to the parent workflow.

## Findings and actual references

All source references below refer to `7b2da402`.

| State | Evidence | Consequence for this plan |
|---|---|---|
| Current | `core/crates/omegon-skills/src/lib.rs`: `SkillManifest`, `SkillProvenance`, `parse_skill_file`, `validate_activation_metadata`; `disclosure.rs` | Reuse YAML/TOML parsing, provenance, activation diagnostics, and disclosure ownership. |
| Current gap | `lib.rs:49-52,130-141`: tolerant recovery defaults malformed or absent frontmatter; `version` is documented as unenforced at lines 62-64 | Parse success and manifest version do not establish validity or optimistic concurrency. Add strict promotion diagnostics in the same owner. |
| Current | `core/crates/omegon/src/features/skills.rs`: `SkillsFeature::execute`, `reload_skills`, `create_skill_file`; `core/crates/omegon/src/skills.rs`: `import_project_skill_guarded` | Create/import/reload and guarded project writes already exist. Extend their composition rather than build an installer. |
| Current gap | `features/skills.rs:339,374-403`; `skills.rs`: import `force` handling | Force is replacement permission, not a precondition token. First slice requires a new project destination and guarded absence checks. |
| Current / contradictory prose | `core/crates/omegon/src/setup.rs:1067-1069` requires operator admission for requested paths; `omegon-skills/src/lib.rs:84-85` and host `skills.rs:37` still describe auto-trust | Executable setup is current. Promotion and activation cannot grant trusted paths. Old comments are not a trust contract. |
| Current gap | `core/crates/omegon/src/command_registry.rs:492-517` registers `/skills`; `control_runtime.rs:265-275` returns `None` for reload and create/import routes | Registry metadata is not create/import remote parity. Freeze and test actual shared promotion dispatch before parallel host work. |
| Current | `core/crates/omegon-memory/src/types.rs:443-542`: `FormationSource`, `FormationEvidence`, `EpisodeFormation`; `formation.rs`: `capture_key`, `advance_capture`, `EpisodeFormation::validate` | Reuse canonical source identities, coverage, bounded excerpts, explicit truncation, and unavailable-source states. |
| Current limit | `types.rs:460-485`: `EvidenceKind::ToolResult` plus `EvidenceOutcome::Succeeded` | The audit shorthand “ToolResultSucceeded” means tool disposition, not a literal type or verified workflow postcondition. Add application and verification identity rather than infer task success. |
| Planned | Existing `MemoryCandidate` at `types.rs:487-494` contains content, section, and evidence IDs | Stable procedure/version identity, distinct application lifecycle, retirement/history, and versioned feedback require new contracts and persistence. |
| Current owners / planned integration | `omegon-memory/src/applicability.rs`, `selection.rs`, `renderer.rs`, `selection_cache.rs`; `omegon-skills/src/disclosure.rs` | Extend whole-unit accounting, environment/policy/evidence cache identity, and lineage deduplication through existing owners. |
| Current accepted evidence | `openspec/archive/2026-09-21-memory-evaluation-corpus/verification-wave-5.md`, `codex-continuation.md:89-116`, and associated reports | Wave 5 smoke passed. All arms had zero repeated-error rubric matches; this cannot demonstrate procedural repeated-error improvement. |
| Unsupported former instruction | No `core/crates/omegon-skills/AGENTS.md` exists at the assessed baseline | Refer to root guidance and real skills owner contracts, plus memory/main/traits directives when editing those crates. |

## Current, planned, and unknown

**Current:** bounded evidence capture, canonical source references, existing
selection infrastructure, skills parsing/provenance/disclosure, host mutation
helpers, operator admission, and accepted Wave 5 evaluation infrastructure.

**Planned:** versioned procedure and application lifecycle, source-bound feedback,
strict promotion diagnostics, guarded plan preconditions, explicit approval,
cross-store operation journal/recovery, shared registered promotion surfaces, and
application-oriented evaluation. All implementation tasks remain unchecked.

**Unknown / decision gates:**

1. Exact promotion command spelling and operator-approval transport across TUI,
   CLI remote, and ACP. Approval must bind the operation and digest; it cannot be
   inferred from an agent tool call. Freeze the shared result DTO before adapters.
2. Final procedure/application/feedback schema and migration identifiers, and the
   explicit Wave 7 verification/validity/retention policy version. Do not wait for
   the whole scheduler or infer policy from retrieval frequency.
3. Durable publication-marker implementation, filesystem durability assumptions,
   and guarded directory-identity checks. Freeze before implementation; matching
   bytes alone cannot prove ownership during recovery.
4. Development-derived quality thresholds and a fresh authorized model budget.
   No new live run or numeric acceptance threshold is authorized by this plan.

Replacement and user/global scope are explicit exclusions, not unresolved defaults
that allow force writes. Any later slice needs its own fenced scope, content-hash
preconditions, operator approval, and human-edit-preserving recovery contract.

## Dependency and parallelization contract

- Wave 5 is accepted as infrastructure and smoke evidence.
- Accepted Wave 6 reconciliation is required for automatic admission/correction.
- Only the explicit Wave 7 contract for evidence versus usage, verification,
  validity, retention, unknown sources, and versioned feedback is required.
- Freeze data interfaces, promotion semantics, shared surfaces, and recovery
  preconditions before parallel host integration.
- After freeze, owner-specific fixtures and detailed design can proceed in
  parallel. Host adapters share DTOs and result semantics rather than inventing
  frontend-local approval or mutation behavior.

## Reconciliation and validation scope

The proposal now names the bounded first slice and dependencies. The design fixes
ownership, lifecycle, authority, preconditions, and forward recovery. The delta
spec supplies observable conflict, replay, unknown-source, applicability, and
quality scenarios. Tasks separate interface gates from implementation and tests.

Run named-change structural validation and `git diff --check` for this plan.
These checks validate documents only. Runtime, persistence, crash recovery,
frontend parity, and model-quality claims remain planned verification work.
