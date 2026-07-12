+++
title = "Repository adoption and lifecycle recommendation policy"
tags = ["0.29","plan","git","lifecycle","openspec","design-tree","ux"]
+++

+++
id = "85ec22e6-2b85-421c-97c5-59e64526e66c"
kind = "design_node"

[data]
title = "Repository adoption and lifecycle recommendation policy"
status = "exploring"
issue_type = "feature"
priority = 2
parent = "plan-refinement"
dependencies = []
open_questions = []
+++

## Overview

# Repository adoption and lifecycle recommendation policy

# Repository adoption and lifecycle recommendation policy

## Overview

Define the 0.29 policy and shared projection for escalating work from an ephemeral conversation into the least costly durable substrate justified by evidence. The first breakpoint is repository adoption: when unversioned work needs history, rollback, review, or cross-session continuity, Omegon recommends initializing a local Git repository. Design nodes and OpenSpec remain independent, later recommendations for architectural reasoning and durable behavioral contracts.

This node refines [[plan-refinement]]. Session plans remain session-owned execution subresources. Repository, design, and specification artifacts are optional bindings; none replaces a session plan's identity.

## Decisions

### Recommend the least costly durable substrate

**Status:** accepted

Recommendations are independent rather than a single maturity ladder:

1. Session plan for structured execution.
2. Local Git repository for versioned files, rollback, review, or multi-session continuity.
3. Design node for durable architectural decisions, assumptions, alternatives, and research.
4. OpenSpec change for shared behavioral contracts, scenario verification, coordination, or release-spanning implementation.
5. Claims, leases, branches, or worktrees for concurrent ownership.

A recommendation at one layer does not imply adoption of later layers.

### Git adoption is the first durability breakpoint

**Status:** accepted

When the workspace is not already inside a Git repository and planned mutation becomes durable work, Omegon may offer to initialize a local repository. Initialization is always explicit, creates no remote, performs no push, and does not create design/OpenSpec artifacts.

### Existing repositories do not imply OpenSpec

**Status:** accepted

Most repository work should remain `Git + session plan + tests + commit`. Item count, file count, research, design, or validation intent alone does not justify OpenSpec. OpenSpec requires evidence of a durable behavioral contract, coordination boundary, verification matrix, public interface, or release-spanning work.

### Recommendations use independent evidence and dispositions

**Status:** accepted

The shared assessment returns independent repository, design, specification, and coordination recommendations with reasons. Each recommendation records disposition: unassessed, recommended, declined until material change, snoozed to a threshold, or adopted/bound.

### Operator agency is mandatory

**Status:** accepted

Recommendations are checkpoints, not automatic transitions. Omegon previews mutations and asks for a decision. No heuristic may initialize Git, create lifecycle artifacts, switch routes, or mutate durable task state without explicit approval.

### Session plans remain session-owned after adoption

**Status:** accepted

Repository adoption adds a workspace binding and permits commit evidence. Design/OpenSpec adoption adds artifact bindings. The plan remains addressed by `(session, plan_index)` and preserves local operational steps that do not belong in durable task files.

### Cross-surface semantics are shared

**Status:** accepted

Assessment and checkpoint actions live in semantic plan/lifecycle projections. TUI, ACP, daemon, and web surfaces consume the same recommendation state. Non-interactive surfaces report a pending checkpoint rather than blocking or silently accepting it.

### Re-prompt only after material evidence changes

**Status:** accepted

A decline or snooze suppresses repetition until an evidence revision changes materially: more mutation scope, another session resume, a public contract, a new contributor/workstream, a release boundary, or newly unresolved architectural decisions.

## Proposed model

```rust
struct WorkDurabilityAssessment {
    evidence_revision: u64,
    repository: Recommendation<RepositoryReason>,
    design: Recommendation<DesignReason>,
    specification: Recommendation<SpecificationReason>,
    coordination: Recommendation<CoordinationReason>,
}

struct Recommendation<R> {
    strength: RecommendationStrength,
    reasons: Vec<R>,
    disposition: RecommendationDisposition,
}

enum RecommendationDisposition {
    Unassessed,
    Recommended,
    DeclinedUntilMaterialChange { revision: u64 },
    Snoozed { threshold: RecommendationThreshold },
    Adopted { binding: ArtifactBinding },
}
```

Repository reasons include unversioned multi-file mutation, destructive migration, dependency upgrades, rollback value, generated replacement of existing files, session resume, and requested commit/checkpoint. Design reasons include unresolved consequential decisions, assumptions, alternatives, and architectural ownership boundaries. Specification reasons include public APIs/schemas/protocols, persistent data models, cross-component acceptance criteria, scenario verification, coordination, and release-spanning requirements.

## Operator checkpoints

### Repository adoption

```text
This work would benefit from versioned history:
• 5 unversioned files may change
• rollback would be useful

[Initialize local Git] [Keep unversioned] [Remind on material growth]
```

### Design capture

```text
This task now has unresolved architectural decisions.
[Create design node] [Keep in session] [Later]
```

### OpenSpec adoption

```text
This change now affects a durable behavioral contract across components.
[Create OpenSpec] [Keep repository-local] [Later]
```

## Dependencies

- [[plan-refinement]] — session plan ownership, registry, projections, and bindings.
- Git repository/workspace identity and safe initialization services.
- Shared command/checkpoint projection used by TUI, ACP, daemon, and web surfaces.
- Design-tree and OpenSpec creation services for optional later adoption.
- Evidence/revision tracking for bounded re-prompt behavior.

## File Scope

### Core policy and state

- `core/crates/omegon/src/plan.rs` — replace coarse promotion nudges with independent recommendation assessment and dispositions.
- `core/crates/omegon/src/conversation.rs` — persist recommendation state/evidence revision with session plans.
- `core/crates/omegon/src/surfaces/` — shared checkpoint/recommendation DTOs and actions.

### Repository assessment and adoption

- `core/crates/omegon/src/git.rs` or `core/crates/omegon-git/` — repository containment, nested-repository detection, safe local initialization plan, and workspace binding.
- `core/crates/omegon/src/tools/` — preview/execute actions through existing authority controls; no shell interpolation.
- Settings only if thresholds require operator-configurable defaults.

### Surfaces

- `core/crates/omegon/src/tui/workbench.rs` and command surfaces — compact checkpoint presentation and decision actions.
- `core/crates/omegon/src/acp_plan_tasks.rs` — pending checkpoint projection and explicit action capability.
- Daemon/web semantic projections — non-blocking pending recommendation state.

### Lifecycle adapters

- Design-tree service — preview/create/bind design nodes.
- OpenSpec service — preview/create/bind changes without replacing session plans.

## Implementation Phases

### Phase 0 — 0.28 semantic correction

Remove or neutralize the current coarse `durable-work` nudge that recommends design/OpenSpec merely because research, design, validation, or evidence-required items exist. Preserve specific evidence-preservation guidance. Do not add a new interactive flow in 0.28.

### Phase 1 — assessment kernel

- Add independent recommendation types, reasons, strength, evidence revision, and disposition.
- Detect Git containment without mutation.
- Separate strong signals from cumulative weak signals.
- Unit-test unversioned codebases, existing repositories, non-filesystem tasks, risky short plans, large mechanical plans, and nested repositories.

### Phase 2 — semantic checkpoint projection

- Project pending recommendations and actions through shared surfaces.
- Persist decline/snooze/adoption disposition with the owning session plan.
- Re-arm only after material evidence revision.
- Ensure automation surfaces never block waiting for an interactive choice.

### Phase 3 — safe local repository adoption

- Produce a preview describing target root, parent/nested repository conditions, initial branch behavior, ignored/generated content risks, and exact mutations.
- Initialize locally only after approval.
- Create no remote and perform no push.
- Do not create an initial commit unless separately requested/approved.
- Attach a workspace binding to the session; keep plan identity unchanged.

### Phase 4 — independent design/OpenSpec recommendations

- Add design reasoning based on unresolved architecture, not task size.
- Add OpenSpec reasoning based on contracts, scenarios, coordination, and release scope.
- Preview artifact creation and bind the existing session plan after approval.
- Preserve artifact revision in bindings and surface drift without automatic write-through.

### Phase 5 — cross-surface and adversarial verification

- TUI, ACP, daemon, and web projection parity.
- Session resume, decline/snooze, nested repo, worktree, parent repo, bare repo, and no-filesystem cases.
- Multi-session/multi-developer behavior without shared mutable session plans.
- Security review for path authority, symlink containment, and Git process spawning.

## Constraints

- Do not make every growing plan require Git, design-tree, or OpenSpec.
- Do not infer repository ownership from a path alone; distinguish repository identity from checkout/worktree path.
- Do not initialize inside an existing parent repository or create nested repositories without explicit disclosure.
- Do not create remotes, push, publish, or configure forge credentials during local adoption.
- Do not create an initial commit implicitly.
- Do not turn item count into an OpenSpec policy.
- Do not write session IDs into shared design/OpenSpec documents merely to build activity views.
- Do not let TUI-specific prompts become hidden blockers for ACP/daemon execution.
- Use argument-array process execution and existing authority controls for Git operations.
- Preserve backward compatibility for sessions without recommendation state.

## Assumptions and Open Questions

- [assumption] Existing Git abstractions can expose containment and initialization planning without shelling through arbitrary command strings.
- [assumption] Recommendation disposition belongs to the session plan, while user-level permanent preferences belong to settings.
- [assumption] Repository initialization is reversible enough to preview as a bounded mutation, but deleting `.git` is not offered as automatic rollback.
- What exact service owns repository identity across clones and worktrees in the first slice?
- Should local initialization default to the configured Git default branch or a fixed Omegon default?
- Which material-change signals increment evidence revision, and which noisy observations are ignored?
- Should `Keep unversioned` last only for the current plan or for the workspace until explicitly reset?
- What minimum ACP action contract is required for approving or declining a checkpoint?

## Acceptance Criteria

- A bounded session plan in an existing repository produces no generic OpenSpec recommendation.
- A large mechanical plan in an existing repository does not recommend OpenSpec solely due to size.
- An unversioned multi-file mutation can produce a repository-adoption recommendation with evidence.
- A public protocol/schema change can recommend OpenSpec independently of plan size.
- Unresolved architecture can recommend a design node independently of Git/OpenSpec state.
- Declining or snoozing suppresses repeat prompts until material evidence changes.
- Approving local Git adoption creates no remote, push, lifecycle artifact, or implicit commit.
- Session plan identity remains `(session, plan_index)` after repository/design/OpenSpec binding.
- Non-interactive surfaces project pending decisions without blocking execution.
- Existing session snapshots deserialize with no active recommendation by default.

## Open Questions
