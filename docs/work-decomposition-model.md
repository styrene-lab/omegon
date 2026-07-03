+++
id = "5c2236a5-fa49-484c-a8da-12323920de14"
kind = "document"
title = "Work decomposition model — beyond the cleave/execute dichotomy"
status = "exploring"
tags = ["architecture", "cleave", "core"]
aliases = ["work-decomposition-model"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
issue_type = "epic"
open_questions = ["[assumption] The three-mode model (Decompose/Delegate/Hydra) is sufficient — there's no need for a fourth mode between Delegate (single child) and Decompose (planned batch). Is this true, or do developers want an ad-hoc multi-child mode without upfront planning?"]
priority = "1"
+++

# Work decomposition model — beyond the cleave/execute dichotomy

## Overview

The current model offers two choices: execute in-session or cleave into parallel worktree children. This binary is showing cracks — cleave has high infrastructure overhead (worktrees, branches, merge, submodule issues), while in-session execution hits context limits on large changes. The test-architect design already implies a richer model (analysis phase → implementation phase). What's the right decomposition spectrum?

## Research

### Evidence: the binary model's failure modes from this session alone

**Vault-secret-backend cleave (complexity 4.5 → cleave)**:\n- 5 children planned, 2 completed in wave 0, 3 never dispatched (max_parallel interaction)\n- Both completed children wrote to wrong paths (stale task file paths)\n- Both completed children couldn't commit (submodule boundary)\n- Net result: full cleave infrastructure spun up, 7 minutes wall time, zero usable merged code\n- We ended up doing 100% of the work in-session after salvaging one child's vault.rs from a dirty worktree\n\n**Cleave improvement implementation (complexity 12 → cleave recommended)**:\n- We ignored the recommendation and executed in-session because all three changes touch orchestrator.rs\n- Parallel children would have produced merge conflicts on the same file\n- Sequential in-session execution took ~20 minutes and produced clean code\n\n**Assessment algorithm blind spots**:\n- `systems × (1 + 0.5 × modifiers)` counts SYSTEMS not FILE CONFLICTS. 8 systems across 2 files → score 12 → recommends cleave → guaranteed merge conflict\n- No consideration of submodule boundaries, worktree limitations, or scope overlap\n- No consideration of whether the task is inherently sequential (e.g., fix → test → fix more → retest)\n- 'execute' means 'do it all yourself right now'. 'cleave' means 'full parallel infrastructure'. No middle ground."

### The missing middle: a decomposition spectrum

The real decision isn't 'cleave or not'. It's 'what decomposition strategy fits this work'. Five modes on a spectrum:\n\n### 1. Direct execution\nDo it now in this session. Single context, sequential, no git overhead. Best for: focused changes, single-file edits, quick fixes, anything that fits in context.\n\n### 2. Phased execution\nStay in-session but break work into explicit phases with compaction between them. 'First implement the client, compact, then implement the recipe kind, compact, then tests.' The agent manages its own context budget. Best for: medium tasks that are sequential but too large for one context window. This is what we actually did for the vault work.\n\n### 3. Lightweight delegation\nSpawn a single child process for a bounded subtask (like the test-architect), wait for it, consume its output. No worktrees, no branches. The child works in a temp directory or reads the repo read-only. Best for: analysis passes, code generation, test plan creation — anything where the output is a file, not a git commit.\n\n### 4. Sequential children\nSpawn children one at a time, each building on the previous one's committed work. Like cleave waves but with wave size 1 and explicit checkpoints. The parent reviews each child's output before dispatching the next. Best for: dependent task chains where later work depends on earlier output.\n\n### 5. Parallel cleave (current model)\nFull worktree isolation, parallel dispatch, merge. Best for: truly independent scope partitions with no file overlap. The actual sweet spot for this is narrow: 3-5 children working on different directories with zero shared files.\n\n### What's missing from the algorithm\nThe current assessment asks 'how complex is this?' It should ask:\n- **Are the scopes overlapping?** If children will edit the same files → mode 2 or 4, not 5\n- **Is there a dependency chain?** If task B needs task A's output → mode 4, not 5\n- **Does the infrastructure support it?** Submodules, monorepos, large binary files → mode 2 or 3\n- **What's the context budget?** If the work fits in one context window → mode 1\n- **Is there analysis work separable from implementation?** → mode 3 for analysis, then mode 4/5 for impl"

### What the test-architect design already implies

The test-architect is a mode 3 (lightweight delegation) feeding into mode 5 (parallel cleave). We're already breaking out of the binary. The pattern generalizes:\n\n- **Pre-flight analysis**: test-architect, dependency discovery, scope conflict detection\n- **Parallel implementation**: current cleave for non-overlapping scopes\n- **Post-merge verification**: coverage check, spec validation\n\nThis is a pipeline, not a binary choice. The assessment should output a STRATEGY, not just 'cleave' or 'execute'. The strategy might be:\n\n```\nStrategy: phased-parallel\nPhase 1: test-architect (lightweight delegation, 30s)\nPhase 2: vault-client + vault-guards (parallel cleave, independent scopes)\nPhase 3: vault-recipe + vault-tui + vault-integrations (parallel cleave, depends on phase 2)\nPhase 4: coverage check (deterministic, <1s)\n```\n\nOr for the cleave-improvements work:\n```\nStrategy: phased-execution\nPhase 1: worktree.rs changes (submodule handling)\nPhase 2: context.rs (new module, depends on worktree.rs API)\nPhase 3: orchestrator.rs (wiring, depends on both)\nPhase 4: progress.rs (independent, could parallel with 3)\nCompact between phases.\n```\n\nThe strategy is the plan. The assessment produces it, not just a yes/no."

### Algorithm redesign: scope-graph analysis replaces system counting

The current formula `systems × (1 + 0.5 × modifiers)` is a proxy for 'how hard is this'. It counts nouns in the directive. It should instead analyze the SCOPE GRAPH:\n\n1. **Parse the plan's file scopes** (from OpenSpec tasks.md or the plan_json)\n2. **Build a conflict graph**: edges between children that share files\n3. **Compute maximum independent set**: children that CAN run in parallel without conflicts\n4. **Detect sequential dependencies**: children where B's scope includes A's output\n5. **Check infrastructure constraints**: submodules, monorepo boundaries, worktree limitations\n\nFrom the graph, derive the strategy:\n- Independent set size ≥ 3 and no infrastructure constraints → parallel cleave\n- Independent set size < 3 but tasks are separable → sequential children or phased execution\n- High file overlap → phased execution (stay in-session)\n- Single file → direct execution\n- Analysis + implementation separable → lightweight delegation then parallel\n\nThis replaces the pattern-matching heuristic with a structural analysis that can't be fooled by how the directive is worded. It also naturally discovers the wave structure instead of requiring the operator/agent to specify depends_on manually."

### Industry-standard complexity and decomposition models

Three established frameworks map directly to our problem:\n\n### 1. Design Structure Matrix (DSM)\nA square matrix where rows/columns are modules and cells represent dependencies. **Partitioning algorithms** reorder the matrix to cluster tightly-coupled modules together and push cross-cutting dependencies above the diagonal. The partitioned DSM directly reveals:\n- **Independent clusters**: modules with no inter-cluster dependencies → parallel execution candidates\n- **Cycles**: mutual dependencies that MUST be handled together → same child or sequential\n- **Bus modules**: modules referenced by everything → foundation layer, build first\n\nThis is exactly the scope-graph analysis I described, but with 30+ years of tooling and theory behind it. Tools like NDepend, IntelliJ, and CppDepend already generate DSMs from code. We don't need to invent the structure — we need to build one from the scope graph and apply standard partitioning.\n\n**Key insight**: DSM partitioning produces the wave structure automatically. Instead of the operator or agent specifying `depends_on`, the system derives waves from the dependency matrix.\n\n### 2. Critical Path Method (CPM)\nOnce we have a task dependency graph with estimated durations, CPM identifies the longest path (bottleneck) and the float (slack) on non-critical tasks. This tells us:\n- **What must be sequential** (critical path = wave dependencies)\n- **What can absorb delays** (float > 0 = safe to parallelize)\n- **Where to invest effort** (reducing critical path duration = faster overall completion)\n\nCPM maps to cleave wave scheduling: critical path tasks go first, float tasks can be deferred or parallelized.\n\n### 3. Coupling/Cohesion Metrics\nEstablished metrics (afferent/efferent coupling, instability index, cohesion measures) give us the structural signal the current algorithm lacks:\n- **High coupling between two scopes** → merge into same child (avoid merge conflicts)\n- **High cohesion within a scope** → good child boundary\n- **Instability index** (Ce / (Ca + Ce)) → unstable modules change often, stable modules are foundation\n\nThese can be computed from the codebase structure at `/init` time and cached as part of the project's structural model."

### /init as structural pre-computation — the bootstrap problem

The DSM and coupling metrics need a structural model of the codebase. This model should be built at `/init` time (or `/migrate`) and stored as part of the project's memory/configuration. It answers questions the assessment algorithm needs:\n\n**What /init currently does**:\n- Scans for AGENTS.md, language config files (Cargo.toml, package.json, etc.)\n- Seeds initial memory facts about the project\n- Detects toolchain (Rust, TypeScript, Python)\n\n**What /init should also do**:\n1. **Module graph**: Walk the crate/package/module structure. For Rust: parse Cargo.toml workspace members, `mod` declarations, `use` imports. For TS: parse tsconfig references, import graph. For Python: parse package structure, imports.\n2. **Dependency matrix**: Build a lightweight DSM from the module graph. Store as a JSON adjacency list (not a full matrix — sparse representation for large codebases).\n3. **Structural landmarks**: Identify bus modules (referenced by >50% of others), leaf modules (referenced by nothing), cycle groups (mutually dependent). Store as memory facts.\n4. **Submodule map**: Detect git submodules, their paths, and which modules live inside them.\n5. **Test infrastructure**: Detect test framework, test file patterns, existing test count per module.\n\nThis pre-computed structure is what the assessment algorithm consumes. When the operator says 'implement vault secret backend', the system looks up the affected modules in the DSM, finds the dependency clusters, computes the critical path, and outputs a strategy — not a napkin-math complexity score.\n\n**For /migrate**: The scan is the same but triggered as part of migration from another tool. The structural model is the first thing built, before migrating sessions or configuration.\n\n**For mid-project drop-in**: If no /init has been run, the assessment falls back to the current heuristic (pattern matching + system counting). The structural model is opportunistically built on first cleave or first OpenSpec change. This makes it progressive — bad data is better than no data, and the model improves over sessions."

### Proposed algorithm: DSM-partitioned strategy with CPM scheduling

```\nInput:\n  - directive (text)\n  - scope (file paths, from OpenSpec or inferred)\n  - structural_model (from /init, optional)\n  - infrastructure_constraints (submodules, monorepo layout)\n\nStep 1: SCOPE RESOLUTION\n  If scope provided (OpenSpec): use it directly\n  If no scope: infer from directive + structural model\n    - Pattern match directive to modules (current heuristic, but output is modules not a score)\n    - If no structural model: fall back to file path extraction from directive text\n\nStep 2: DEPENDENCY ANALYSIS\n  Build scope-DSM: NxN matrix where N = scope modules\n  For each pair (A, B):\n    - Direct dependency: A imports/uses B → cell = 1\n    - Shared file: A and B modify same file → cell = 2 (merge conflict risk)\n    - Submodule boundary: A and B in different git repos → flag\n  Source: structural model if available, else quick `grep` pass over scope files\n\nStep 3: DSM PARTITIONING\n  Apply Tarjan's algorithm to find strongly-connected components (cycles)\n  Merge cycles into single work units (can't parallelize within a cycle)\n  Topological sort remaining DAG → natural wave order\n  Identify bus modules (high afferent coupling) → foundation wave\n\nStep 4: STRATEGY SELECTION\n  Metrics from the partitioned DSM:\n    - parallel_groups: number of independent clusters after partitioning\n    - max_wave_depth: longest dependency chain\n    - conflict_density: ratio of shared-file edges to total edges\n    - total_scope_size: number of files\n\n  Decision matrix:\n    total_scope_size ≤ 3 → DIRECT EXECUTION\n    conflict_density > 0.5 → PHASED EXECUTION (too many shared files for parallel)\n    parallel_groups ≤ 1 → SEQUENTIAL CHILDREN (everything depends on everything)\n    parallel_groups ≥ 2 AND conflict_density ≤ 0.3 → PARALLEL CLEAVE\n    analysis separable from implementation → LIGHTWEIGHT DELEGATION + one of above\n\nStep 5: PLAN GENERATION\n  Output a strategy plan:\n    phases: [{mode, children, estimated_effort}]\n    critical_path: [module names in order]\n    infrastructure_warnings: [submodule, merge risk, etc.]\n    estimated_total_time: CPM calculation\n```\n\nThis replaces `systems × (1 + 0.5 × modifiers) > 2.0 → cleave`. The output is a plan, not a boolean."

### Memory system as the structural model — not a parallel store

The memory system already has everything the DSM needs:\n\n### What exists today\n- **2230 active facts** across Architecture (349), Decisions (504), Constraints (385), Known Issues (330)\n- **Edges table** with relation, confidence, decay, bidirectional indexing — fully schema'd, zero edges populated\n- **Semantic search** via embeddings (memory_recall) — can find facts relevant to any scope\n- **FTS5 full-text search** as fallback\n- **Global extraction pipeline** that already supports `connect` actions to create edges\n- **Cross-session persistence** via facts.jsonl sync\n\n### What's missing: structural facts\nThe memory system stores KNOWLEDGE facts ('Vault client lives in omegon-secrets'). It doesn't store STRUCTURAL facts ('omegon-secrets/vault.rs imports from omegon-secrets/resolve.rs'). The structural model from /init should be stored AS memory facts in a dedicated section (e.g., 'Structure') with edges:\n\n```\nFact A: 'Module: core/crates/omegon-secrets — Rust crate, 6 source files'\nFact B: 'Module: core/crates/omegon — Rust crate, binary, 30+ source files'\nEdge: A →[depends_on]→ B (description: 'omegon depends on omegon-secrets')\n\nFact C: 'File: core/crates/omegon-secrets/src/vault.rs — VaultClient, 800 LoC'\nFact D: 'File: core/crates/omegon-secrets/src/resolve.rs — resolve_secret, recipes'\nEdge: D →[imports]→ C (description: 'resolve.rs uses VaultClient from vault.rs')\n\nFact E: 'Submodule: core/ → separate git repo'\nEdge: A →[inside_submodule]→ E\nEdge: B →[inside_submodule]→ E\n```\n\n### How the DSM algorithm queries memory\nInstead of building a DSM from scratch on every assessment:\n1. `memory_recall('modules related to vault secret backend')` → finds structural facts\n2. Query edges WHERE source_fact_id IN (found facts) → builds adjacency list\n3. Apply Tarjan's SCC + topological sort → partitioned DSM\n4. Enrich with knowledge facts: `memory_recall('known issues with vault')` → adds risk signals to the strategy\n\n### Three types of edges the structural model needs\n1. **Static dependencies** (`imports`, `depends_on`): from code analysis at /init. Stable, high confidence.\n2. **Co-change relationships** (`changes_with`): from git history. 'vault.rs and resolve.rs are modified together in 80% of commits.' Emergent, medium confidence, decays.\n3. **Knowledge relationships** (`motivated_by`, `contradicts`, `enables`): from extraction. 'The VaultClient decision enables the vault recipe kind.' High-value for understanding WHY, not just WHAT.\n\nType 1 comes from /init scanning. Type 2 comes from git log analysis (also /init). Type 3 comes from the existing extraction pipeline. All three store as edges in the same graph.\n\n### Why this is better than a separate structural store\n- **Single query interface**: memory_recall finds both structural AND knowledge facts\n- **Confidence decay**: stale structural facts (from an old /init run) naturally decay\n- **Cross-session**: structural model persists without a separate file\n- **Incremental updates**: each session's extraction can add new structural facts from code it reads\n- **Already exists**: the edges table, the decay model, the query interface — all built, just empty"

### Co-change analysis from git history — the emergent coupling signal

Static import analysis misses emergent coupling. Two files that never import each other but are always modified together are coupled — by shared assumptions, shared data schemas, or shared behavioral contracts. Git history reveals this:\n\n```bash\ngit log --name-only --format='' --diff-filter=M -- '*.rs' | \\\n  awk '/^$/{next} {files[NR]=$0} END{...}' # pairwise co-change frequency\n```\n\nThis produces edges like:\n```\nvault.rs ←[changes_with, confidence=0.8]→ resolve.rs\nlib.rs ←[changes_with, confidence=0.6]→ Cargo.toml\nmod.rs ←[changes_with, confidence=0.9]→ orchestrator.rs\n```\n\nCo-change coupling is MORE predictive than import coupling for merge conflicts. If two files change together 80% of the time, putting them in different cleave children guarantees conflicts. The DSM should weight co-change edges higher than import edges for the conflict_density calculation.\n\n`/init` can compute the co-change matrix from git history (last 100 commits, say) and store as memory edges with `changes_with` relation. This decays naturally — old co-change patterns fade as the code evolves.\n\nThis is exactly the 'Design Structure Matrix approach for measuring co-change-modularity' from the ACM research (Zimmerman et al.) — using association rule mining on co-change graphs to determine evolutionary coupling."

### Root cause: why edges haven't populated

Three independent failures:\n\n### 1. Global extraction disabled by default (the kill switch)\n`globalExtractionEnabled: false` in DEFAULT_CONFIG (types.ts line 97). Added March 4th to suppress 429 rate limit noise. Defaulted to false. The global store has 35 active edges from sessions BEFORE the disable — proving the system worked when it was on. The 954 archived edges show decay working correctly over time.\n\n**Fix**: Default to true. Handle 429s gracefully (already done — the catch block silently skips on rate limit). The fix that disabled global extraction was a valid response to noise, but the default should have been true with the rate limit handling as the actual fix.\n\n### 2. Project extraction (Phase 1) never produces connect actions\nThe Phase 1 prompt only supports observe/reinforce/supersede/archive. It has no `connect` action type. Edges are exclusively a Phase 2 (global) concern. This means even when global extraction runs, it only connects GLOBAL facts — never project-specific structural relationships like 'vault.rs depends on resolve.rs'.\n\n**Fix**: Add `connect` as a valid action in Phase 1 project extraction. The prompt already has access to fact IDs. Adding a connect action type lets it create project-level edges during normal extraction.\n\n### 3. memory_connect tool is available but never proactively used\nThe tool exists, works, and can write to both project and global stores. But the agent's system prompt doesn't encourage its use. The prompt guidelines say 'Use memory_connect to create relationships between facts' but don't describe WHEN to do it or what relationships are valuable.\n\n**Fix**: Add explicit guidance: 'After storing architectural facts, connect them with depends_on/imports/enables edges. After discovering co-change patterns, connect with changes_with edges.' And for /init: systematically create structural edges from code analysis."

## Decisions

### Decision: The structural model IS the memory graph — /init populates structural facts and edges, the DSM algorithm queries memory

**Status:** decided
**Rationale:** Building a separate structural store would duplicate the persistence, query, decay, and cross-session sync infrastructure that already exists in the memory system. The edges table is empty but fully schema'd with relation, confidence, decay, and bidirectional indexes. Structural facts (modules, files, submodules) go in a 'Structure' section. Three edge types: static imports (from /init code scan), co-change coupling (from /init git log analysis), and knowledge relationships (from existing extraction). The DSM algorithm queries memory_recall for scope-relevant structural facts, fetches edges for adjacency, and applies standard partitioning. This means every session that reads and modifies code can incrementally improve the structural model via the existing extraction pipeline — the model gets better over time without explicit /init re-runs.

### Decision: Strategy output replaces binary decision — assessment returns a phased plan with modes, not cleave/execute

**Status:** decided
**Rationale:** The evidence from this session is conclusive: the binary model produces wrong recommendations. The DSM-partitioned strategy naturally produces the right decomposition mode because it's based on structural analysis, not keyword counting. The strategy includes phases, modes per phase, critical path, and infrastructure warnings. Backward compatibility: the strategy includes a top-level `decision` field that maps to cleave/execute for old callers.

### Decision: DSM from memory graph replaces pattern-matching heuristic — patterns become fallback when no structural model exists

**Status:** decided
**Rationale:** The current pattern library (12 domain patterns + modifier detection) becomes the cold-start fallback. When the memory graph has structural facts and edges, the DSM algorithm supersedes it. The pattern heuristic is still useful for the first session before /init populates structure. Progressive enhancement: cold start → pattern heuristic → structural DSM → DSM + co-change.

### Decision: Migration via backward-compatible strategy envelope — strategy.decision provides cleave/execute for old callers, strategy.phases for new callers

**Status:** decided
**Rationale:** The cleave_assess tool return type gains a `strategy` field alongside the existing `decision`, `complexity`, etc. Old callers that read `decision` still work. New callers read `strategy.phases`. The `decision` field is derived from the strategy: if any phase uses parallel cleave mode → 'cleave'; otherwise → 'execute'. This is a non-breaking additive change to the tool API.

### Decision: Phased execution is guidance injected into system prompt — the strategy's phases become a work plan the in-session agent follows with compaction checkpoints

**Status:** decided
**Rationale:** No new infrastructure needed. The strategy output includes phases with estimated effort. When the top-level mode is 'phased execution', the system prompt gains a section: 'Work Plan: Phase 1: [files], Phase 2: [files]. Compact between phases.' The agent follows this plan, calling memory_compact between phases. This is how we actually worked in this session — we just did it manually. The strategy makes it explicit.

## Open Questions

- [assumption] The three-mode model (Decompose/Delegate/Hydra) is sufficient — there's no need for a fourth mode between Delegate (single child) and Decompose (planned batch). Is this true, or do developers want an ad-hoc multi-child mode without upfront planning?

## Upstream Evidence Update — VCS-backed Workstream Perforation

### Research notes

This section refines the decomposition model with upstream evidence from established dependency modeling and VCS workflows.

#### Design Structure Matrix supports the scope-graph direction

The DSM literature describes a Design Structure Matrix as a square matrix for representing and analyzing relationships between elements in a system. DSMWeb's introduction specifically calls out clustering to facilitate modularity and sequencing to minimize process cost/schedule risk. It also describes task DSMs where columns represent task inputs and rows represent task outputs.

Implication for Omegon: the planned scope graph should be treated as a sparse DSM over repo elements and task streams. The assessment should cluster tightly coupled files/modules into one workstream and sequence dependent clusters into waves. Parallelism is justified by low coupling between clusters, not by textual complexity alone.

#### Git worktree is an upstream-supported substrate for multiple repo-local workspaces

The Git documentation describes `git worktree` as managing multiple working trees attached to the same repository. It explicitly states that a repository can support multiple working trees, allowing more than one branch to be checked out at a time, with linked worktrees carrying additional metadata distinct from the main worktree.

Implication for Omegon: git worktrees are not a hack around Git; they are the correct Git-native substrate for child workstreams inside the bounds of an existing repository. The harness should model them explicitly as child workspaces with lifecycle, scope, branch/ref, status, merge, and cleanup state.

#### Jujutsu workspaces fit the same abstraction and may be a better multi-workstream substrate

Jujutsu documentation describes each workspace as having a different commit checked out and notes multiple workspaces can be useful for running long-running tests in one workspace while continuing development in another. Jujutsu also records working-copy changes as commits and exposes operation-log restore/undo semantics. Its conflict documentation treats conflicts as first-class states that can be materialized in the working copy and diff output.

Implication for Omegon: the harness should not hard-code Git worktree semantics into cleave. It should define a VCS substrate abstraction with Git and jj implementations. jj's change/workspace/operation model is a natural fit for private bounded workstreams, stacked changes, conflict-state preservation, and parent-governed integration.

#### Branching/integration literature warns against long-lived, poorly integrated branches

Branching-pattern guidance emphasizes integration frequency, integration friction, and modularity. Feature-toggle/trunk-based discussions warn that long-lived branches create painful merges and recommend techniques that keep integration frequent and controlled.

Implication for Omegon: child workstreams should be short-lived, bounded, and merged/harvested by the parent as soon as their acceptance contract is satisfied. Cleave should avoid producing long-running orphan branches. If a task cannot be decomposed into short-lived low-conflict streams, the assessment should choose phased execution or sequential children instead of parallel cleave.

### Refined design claim

A confident `cleave_assess` should produce **VCS-backed workstream perforation lines**: explicit corpus/domain boundaries where a child can work privately with low integration risk.

The lifecycle is:

```text
directive
  -> scope resolution
  -> sparse DSM / dependency graph
  -> cluster + sequence analysis
  -> perforation lines
  -> private git/jj workspaces
  -> child execution
  -> parent merge/harvest
  -> parent validation and synthesis
```

This makes parallelism a repo-structural conclusion rather than a heuristic reaction to complexity.

### Perforation line contract

```rust
struct PerforationLine {
    id: String,
    domain: String,
    rationale: String,
    write_scope: Vec<PathSpec>,
    read_scope: Vec<PathSpec>,
    forbidden_scope: Vec<PathSpec>,
    depends_on: Vec<String>,
    acceptance: Vec<String>,
    validation: Vec<ValidationCommand>,
    conflict_risk: ConflictRisk,
    confidence: f32,
}
```

Rules:

- `write_scope` is the child authority boundary.
- `read_scope` is context, not mutation authority.
- `forbidden_scope` captures known conflict-sensitive or parent-owned paths.
- `depends_on` becomes wave scheduling input.
- `acceptance` and `validation` must be concrete enough for parent synthesis.
- `conflict_risk` is computed from shared paths, dependency edges, bus modules, generated artifacts, and VCS substrate constraints.

### VCS substrate contract

```rust
enum WorkstreamVcsSubstrate {
    GitWorktree {
        path: PathBuf,
        branch: String,
        base_commit: String,
    },
    JjWorkspace {
        path: PathBuf,
        workspace: String,
        base_change: String,
    },
}

struct ChildWorkspacePlan {
    workstream_id: String,
    substrate: WorkstreamVcsSubstrate,
    write_scope: Vec<PathSpec>,
    merge_target: ParentWorkspaceRef,
    cleanup_policy: CleanupPolicy,
}
```

The parent harness owns:

- substrate selection;
- child workspace creation;
- scope enforcement or boundary-violation reporting;
- child result harvest;
- merge/rebase/integration;
- parent validation;
- cleanup or retention for debugging.

Children do not merge themselves into the parent branch/change. They produce candidate changes inside bounded private workspaces.

### Assessment confidence ladder

A newer assessment must distinguish heuristic decomposition from evidence-backed decomposition.

| Level | Method | Result |
|---|---|---|
| 0 | Keyword heuristic only | Low confidence; may recommend investigation, not automatic cleave. |
| 1 | Directive + repo search | Medium confidence candidate scopes; assumptions must be surfaced. |
| 2 | Scope graph / DSM | High confidence if clusters have low overlap and clear dependencies. |
| 3 | Spec-backed scope graph | Highest confidence; OpenSpec/design scenarios map to child acceptance contracts. |

Only levels 2-3 should automatically create private parallel workstreams under normal conservative/orchestrator policy. Levels 0-1 may suggest a plan, but should not be treated as safe parallelism.

### Strategy selection refinement

The old binary `execute` vs `cleave` should remain as a compatibility projection. The real strategy should choose among:

- `direct_execution` — small or tightly coupled work;
- `phased_execution` — large but high-overlap work handled by parent phases;
- `lightweight_delegate` — bounded analysis/verification/generation without merge authority;
- `sequential_children` — child workspaces with dependency depth or high integration risk;
- `parallel_cleave` — independent low-overlap perforation lines with parent-governed merge;
- `hybrid` — e.g. analysis delegate, then sequential foundation, then parallel leaf workstreams.

Decision pressures:

- High shared-file overlap -> phased or sequential, not parallel.
- Bus/foundation modules -> early sequential wave.
- Independent leaf modules with owned tests -> parallel cleave.
- Dirty tree/submodule ambiguity -> checkpoint or avoid parallel substrate until clean.
- Missing acceptance criteria -> assess/design first, not cleave.

### Assessment output shape

Additive, backward-compatible result:

```json
{
  "decision": "cleave",
  "complexity": 4.5,
  "confidence": 0.88,
  "method": "spec_backed_scope_graph",
  "strategy": {
    "mode": "parallel_cleave",
    "rationale": "Three low-overlap repo clusters with one foundation dependency.",
    "substrate": { "preferred": "jj", "fallback": "git_worktree" },
    "perforation_lines": [
      {
        "id": "event-model",
        "domain": "workstream event schema and reducer",
        "write_scope": ["core/crates/omegon/src/workstreams/**"],
        "read_scope": ["core/crates/omegon/src/features/delegate.rs", "core/crates/omegon/src/features/cleave.rs"],
        "forbidden_scope": [],
        "depends_on": [],
        "acceptance": ["event types compile", "unit tests cover reconciliation state"],
        "validation": ["cargo test -p omegon workstream"],
        "conflict_risk": "low",
        "confidence": 0.91
      }
    ],
    "waves": [["event-model"], ["delegate-emission", "workbench-projection"]],
    "parent_obligations": [
      "merge children in wave order",
      "run parent validation after each wave",
      "synthesize child claims against harness-observed diffs/tests"
    ]
  },
  "warnings": []
}
```

### Integration with workstream events

Each perforation line becomes a child workstream in the event model:

```text
cleave_21
  ├─ event-model             git/jj private workspace
  ├─ delegate-emission       git/jj private workspace
  ├─ workbench-projection    git/jj private workspace
  └─ validation              parent-owned
```

The parent receives structured events for:

- workspace created;
- child progress;
- boundary expansion requested;
- child completed/failed/cancelled;
- merge started/conflicted/completed;
- validation passed/failed;
- synthesis ready.

This connects decomposition to the result-continuation design: the harness owns awareness of child completion and merge state; the model interprets and acts.

### Implementation implications

1. Keep current heuristic `decision` as fallback and compatibility output.
2. Add `strategy` and confidence fields to `cleave_assess` before changing callers.
3. Add a lightweight scope graph builder from explicit paths, OpenSpec tasks, design-node impl notes, import scans, and code search.
4. Add conflict graph checks before recommending parallel cleave.
5. Add a VCS substrate trait with Git worktree first and jj workspace support second.
6. Enforce child write scopes at least by diff review initially; later by tool/runtime guardrails.
7. Emit workstream events from substrate lifecycle and merge phases.
8. Add replay tests proving that high-overlap large work selects phased/sequential mode and low-overlap spec-backed work selects parallel cleave.

## Parent-mediated A2A Communication for Cleave Workstreams

### Design position

Cleave child agents are independent Omegon runtimes, but their coordination topology should remain parent-orchestrated by default.

A2A-style communication is valuable as a structured protocol vocabulary for task envelopes, capability declarations, artifacts, progress, cancellation, and status. It should not initially mean freeform peer-to-peer child chat. Direct child-to-child negotiation undermines scope authority, provenance, merge ownership, and deadlock control.

Default topology:

```text
            parent Omegon runtime
          /          |           \
 child workstream  child workstream  child workstream
```

The parent sends task envelopes to children. Children send events, artifacts, blockers, and boundary requests to the parent. The parent may route accepted dependency artifacts or approved questions to downstream children.

### Communication layers

#### Layer 1 — Parent-child task protocol

Required for cleave v2.

```rust
struct AgentRunEnvelope {
    workstream_id: WorkstreamId,
    parent_id: Option<WorkstreamId>,
    objective: String,
    scope: ScopeContract,
    acceptance: Vec<AcceptanceCriterion>,
    dependencies: Vec<DependencyArtifactRef>,
    constraints: Vec<String>,
    expected_artifacts: Vec<ArtifactKind>,
    communication_policy: CommunicationPolicy,
}
```

The envelope is the structured replacement for prompt-only child setup. The child prompt may render from it, but the envelope remains the canonical contract for audit, replay, Workbench projection, and scope enforcement.

#### Layer 2 — Child event stream

Required for progress, auditability, and parent synthesis.

```rust
enum AgentRunEvent {
    Started,
    Progress(ProgressUpdate),
    ArtifactProduced(ArtifactRef),
    BoundaryExpansionRequested(ScopeRequest),
    Blocked(Blocker),
    Completed(ChildResult),
    Failed(FailureReport),
    Cancelled,
}
```

These events should be normalized into the same internal `WorkstreamEvent` store used for delegate result continuations and cleave progress.

#### Layer 3 — Parent-mediated dependency artifacts

Required for dependency waves and safe inter-child knowledge transfer.

```rust
struct DependencyArtifact {
    id: ArtifactId,
    producer: WorkstreamId,
    kind: ArtifactKind,
    trust: TrustLevel,
    content_ref: ArtifactRef,
    accepted_by_parent: bool,
    accepted_at: Option<DateTime<Utc>>,
}
```

A downstream child may consume only parent-accepted artifacts by default. This prevents one child from smuggling unreviewed design decisions into another child's task stream.

Example:

```text
event-model child -> parent:
  ArtifactProduced: WorkstreamEvent schema draft

parent:
  validates compile/tests or reviews schema
  marks artifact accepted

parent -> workbench child:
  DependencyArtifactAvailable: accepted WorkstreamEvent schema
```

#### Layer 4 — Parent-mediated child mailbox

Optional second phase.

```rust
struct WorkstreamMessage {
    id: MessageId,
    from: WorkstreamId,
    to: WorkstreamId,
    via_parent: bool,
    kind: MessageKind,
    content_ref: ArtifactRef,
    requires_parent_approval: bool,
    status: MessageStatus,
}
```

The initial implementation should support `via_parent = true` only. A child may ask the parent to route a bounded question to another child. The parent can approve, deny, answer directly, merge the workstreams, or replan.

#### Layer 5 — Direct child-to-child A2A

Future/advanced only. Direct child-to-child communication should require explicit operation policy because it increases hidden coupling, prompt-injection surface, audit complexity, and deadlock risk.

Allowed only when:

- all messages are captured in the parent event store;
- child scopes are still enforced by the parent harness;
- no child can grant authority to another child;
- the parent receives all artifacts and transcripts;
- timeouts/deadlocks are detected;
- the operation profile explicitly grants collaborative child communication.

### Communication policy

```rust
enum CommunicationPolicy {
    ParentOnly,
    ParentMediatedMailbox,
    DirectPeerWithAudit,
}
```

Default: `ParentOnly`.

Recommended mapping:

| Operation mode | Communication policy |
|---|---|
| direct execution | none |
| lightweight delegate | ParentOnly |
| sequential children | ParentOnly + parent-accepted dependency artifacts |
| parallel cleave | ParentOnly by default; ParentMediatedMailbox if dependency questions are expected |
| collaborative experimental cleave | DirectPeerWithAudit, explicit approval only |

### Integration with `WorkstreamEvent`

A2A-style communications should be first-class workstream events, not side-channel logs.

Additional event kinds:

```rust
enum WorkstreamEventKind {
    // existing kinds omitted
    TaskEnvelopeSent,
    ChildCapabilityAdvertised,
    ChildMessageRequested,
    ChildMessageApproved,
    ChildMessageDenied,
    ChildMessageDelivered,
    ArtifactProduced,
    ArtifactAccepted,
    ArtifactRejected,
    DependencyArtifactDelivered,
    BoundaryExpansionRequested,
    BoundaryExpansionApproved,
    BoundaryExpansionDenied,
}
```

Each event must preserve provenance:

```rust
struct WorkstreamEvent {
    event_id: EventId,
    workstream_id: WorkstreamId,
    parent_id: Option<WorkstreamId>,
    kind: WorkstreamEventKind,
    producer: EventProducer,
    trust: TrustLevel,
    summary: String,
    payload_ref: Option<ArtifactRef>,
    payload_digest: Option<String>,
    created_at: DateTime<Utc>,
    consumed_at: Option<DateTime<Utc>>,
    trace_context: Option<TraceContext>,
}
```

Child output and inter-child messages are untrusted data unless elevated by parent validation. Harness-observed facts such as process status, VCS status, artifact digest, merge result, and validation exit code are higher-trust events.

### Logs, audits, and tracing

The event system must support three distinct consumers.

#### Runtime logs

Purpose: debugging a live operation.

Contents:

- task envelope sent;
- child process/workspace created;
- child progress;
- message routed/denied;
- artifacts produced;
- boundary requests;
- merge/validation events;
- errors/timeouts.

Log records may include summarized payloads, but large artifacts should be referenced by digest/path, not inlined everywhere.

#### Audit trail

Purpose: explain who/what influenced final code.

Minimum audit record:

```rust
struct WorkstreamAuditRecord {
    event_id: EventId,
    operation_id: WorkstreamId,
    child_id: Option<WorkstreamId>,
    producer: EventProducer,
    action: AuditAction,
    authority_basis: Option<AuthorityGrantRef>,
    scope: Option<ScopeContract>,
    artifact_digest: Option<String>,
    vcs_ref: Option<String>,
    timestamp: DateTime<Utc>,
}
```

Audit answers:

- which child received which task envelope;
- what scope authority it had;
- which artifacts it produced;
- which artifacts were accepted by the parent;
- which downstream children consumed those artifacts;
- whether scope boundaries changed;
- which merge/validation results justified final synthesis.

#### Distributed tracing

Purpose: correlate parent and child runtime activity.

Use trace/span semantics:

```text
trace: cleave_21
  span: assess
  span: create-workspaces
  span: child event-model
    span: tool read
    span: tool edit
    span: validation
  span: child workbench
  span: merge wave 1
  span: parent validation
  span: synthesis
```

Each parent-child envelope should carry trace context. Child events should include that trace context when reported back. This makes it possible to render an operation timeline, diagnose slow children, and attribute token/tool usage by child.

### UX/UI projections

The same communication events should project differently by surface.

#### Workbench

Workbench should show communication state compactly:

```text
cleave_21 workstream events                         running
  ✓ event-model                 artifact accepted: schema
  ⏳ workbench-projection        waiting on schema artifact
  ! delegate-emission           boundary request: delegate.rs
```

For mailbox-enabled operations:

```text
  ↪ workbench -> event-model     question pending parent approval
  ↩ event-model -> workbench     answer delivered
```

#### Detail pane / inspector

A selected workstream should expose:

- task envelope;
- scope contract;
- accepted dependency artifacts;
- produced artifacts;
- routed messages;
- boundary requests;
- VCS workspace/ref;
- trace timeline;
- validation evidence.

#### Transcript

The transcript should not show every low-level message by default. It should show milestone summaries and blockers:

```text
Runtime workstream update:
cleave_21 delivered accepted event-model schema to workbench-projection and delegate-emission children.
```

Blockers should be explicit:

```text
Runtime workstream blocker:
delegate-emission requested write access outside its perforation line: core/crates/omegon/src/features/delegate.rs.
Parent decision required: approve expansion, reassign, merge streams, or deny.
```

#### Web/ACP/API

The semantic projection should expose message/artifact state without TUI glyphs:

```json
{
  "operation_id": "cleave_21",
  "children": [...],
  "messages": [...],
  "artifacts": [...],
  "blockers": [...],
  "trace": {...}
}
```

### Parent synthesis obligations

Parent synthesis must account for communications, not just final diffs.

A final cleave summary should answer:

- Which child artifacts influenced which downstream children?
- Were any child messages denied or unresolved?
- Did any child request boundary expansion?
- Were all accepted artifacts validated before downstream consumption?
- Did any child consume unaccepted/untrusted output?
- Which VCS refs/workspaces were merged?
- Which validation events support the final claim?

### Guardrails

- Direct child-to-child communication is disabled by default.
- A child cannot grant another child scope, authority, or merge rights.
- Child messages are untrusted data until parent-accepted.
- Dependency artifacts must be accepted by the parent before downstream use by default.
- Hidden peer dependencies are forbidden; any dependency discovered at runtime becomes a parent-visible event.
- Repeated mailbox chatter is a decomposition smell; the parent should consider merging streams or switching to sequential execution.
- Every routed message and artifact must have an audit record and trace context.

### Implementation path

1. Define `AgentRunEnvelope`, `AgentRunEvent`, `DependencyArtifact`, and `CommunicationPolicy` DTOs.
2. Normalize these DTOs into the existing/new `WorkstreamEvent` store.
3. Emit `TaskEnvelopeSent`, `ArtifactProduced`, `ArtifactAccepted`, and `DependencyArtifactDelivered` events during cleave execution.
4. Add trace context propagation from parent to child Omegon processes.
5. Add audit records for task envelopes, scope contracts, artifacts, boundary requests, and routed messages.
6. Project communication state into Workbench and operation detail surfaces.
7. Add parent-mediated mailbox after artifact routing is stable.
8. Defer direct peer A2A until the audit/tracing/scope-enforcement substrate proves reliable.

## Assessment Target — Cleave Additions

### Target statement

The next cleave additions should make `cleave_assess` capable of producing an evidence-backed decomposition strategy for large tasks, with bounded VCS-backed workstreams when the repo structure supports parallelism.

The target is not to make cleave more eager. The target is to make cleave more selective, more structural, and more executable.

A successful assessment should answer:

1. **Should this work be decomposed?**
2. **What decomposition mode fits?** direct, phased, delegate, sequential children, parallel cleave, or hybrid.
3. **Where are the repo/domain perforation lines?**
4. **Which child workstreams are safe to run in private git/jj workspaces?**
5. **What dependencies, artifacts, and validations govern those workstreams?**
6. **What must remain parent-owned for synthesis, merge, and final claims?**

### Non-goals

- Do not parallelize merely because a task is large.
- Do not use keyword complexity as authority for parallel workstream creation.
- Do not allow child workstreams without concrete write scope and acceptance criteria.
- Do not introduce direct child-to-child A2A as part of the initial cleave assessment work.
- Do not require jj for the first implementation; design the substrate abstraction so jj can be added cleanly after Git worktree support.

### Minimum viable cleave assessment v2

The first implementation target should be additive and backward-compatible.

Keep the existing fields:

```json
{
  "decision": "execute|cleave",
  "complexity": 0.0,
  "method": "...",
  "threshold": 2.0
}
```

Add:

```json
{
  "strategy": {
    "mode": "direct_execution|phased_execution|lightweight_delegate|sequential_children|parallel_cleave|hybrid|needs_design|needs_scope_discovery",
    "rationale": "...",
    "confidence": 0.0,
    "perforation_lines": [],
    "waves": [],
    "parent_obligations": []
  },
  "confidence_breakdown": {
    "scope_resolution": 0.0,
    "dependency_graph": 0.0,
    "conflict_analysis": 0.0,
    "acceptance_criteria": 0.0,
    "vcs_substrate": 0.0,
    "model_judgment": 0.0,
    "overall": 0.0
  },
  "warnings": [],
  "assumptions": [],
  "evidence": []
}
```

Legacy `decision` is a projection from the strategy:

- `parallel_cleave` or `sequential_children` with child workspaces -> `cleave`
- everything else -> `execute`

This preserves existing callers while giving new callers the structural plan.

### Evidence dossier target

`cleave_assess` should build or receive an evidence dossier before model adjudication.

Minimum dossier fields:

```rust
struct AssessmentDossier {
    directive: String,
    explicit_paths: Vec<PathBuf>,
    candidate_files: Vec<CandidateFile>,
    candidate_domains: Vec<CandidateDomain>,
    design_refs: Vec<EvidenceRef>,
    openspec_refs: Vec<EvidenceRef>,
    tests: Vec<TestOwner>,
    vcs: VcsAssessmentState,
    dirty_paths: Vec<PathBuf>,
    submodules: Vec<PathBuf>,
    constraints: Vec<String>,
}
```

Initial evidence sources:

- explicit file paths in the directive;
- codebase search hits for directive terms;
- nearby tests by naming/path convention;
- OpenSpec task groups and scenarios when available;
- design-tree implementation notes when available;
- git dirty state and submodule boundaries;
- existing cleave plan JSON when provided.

Later evidence sources:

- import graph;
- symbol ownership;
- historical conflict/churn data;
- memory-backed structural facts;
- jj operation/workspace state.

### Mechanical gates before parallel cleave

The harness should reject or downgrade parallel cleave when gates fail.

Required for `parallel_cleave`:

- VCS state is clean or checkpointed.
- Worktree/jj substrate is available.
- At least two child workstreams have concrete non-identical write scopes.
- Every child has acceptance criteria.
- Every child has validation guidance or a parent validation fallback.
- Shared write-file conflict risk is low or explicitly sequenced.
- Parent obligations include merge, validation, synthesis, and lifecycle/docs/changelog checks when applicable.
- Autonomy policy allows the requested child count and parallelism, or structured approval is returned.

Downgrade rules:

| Condition | Downgrade |
|---|---|
| no concrete scope | `needs_scope_discovery` |
| acceptance criteria missing | `needs_design` or `phased_execution` |
| high shared-file overlap | `phased_execution` or `sequential_children` |
| one useful side quest | `lightweight_delegate` |
| bus/foundation module first | `sequential_children` or hybrid foundation wave |
| dirty/submodule ambiguity | checkpoint first or avoid parallel substrate |

### Model tier target

Large cleave assessments should use A/S-tier models by default. Local qwen3/sonnet-class models may assist with labeling, summarization, or adversarial checklist passes, but should not be the sole authority for high-confidence parallelism unless the mechanical evidence is overwhelmingly clear.

Recommended critical assessment flow:

```text
harness evidence dossier
  -> S/A-tier planner produces candidate strategy
  -> S/A-tier or strong secondary adversarial review critiques boundaries
  -> harness mechanical gates validate/downgrade
  -> final strategy returned with confidence and executable plan
```

### Parent-mediated communication target

Assessment output must account for communication topology.

For each perforation line, include:

- communication policy: `ParentOnly` initially;
- expected artifacts;
- dependencies on parent-accepted artifacts;
- whether mailbox questions are expected;
- boundary expansion behavior.

Initial policy:

```text
parallel cleave children communicate only through parent workstream events and accepted artifacts.
```

### Acceptance criteria for the cleave additions

The cleave assessment additions are successful when tests/replay fixtures demonstrate:

1. A vague large directive returns `needs_scope_discovery`, not confident cleave.
2. A high-overlap large directive returns `phased_execution` or `sequential_children`, not parallel cleave.
3. A spec-backed low-overlap directive returns `parallel_cleave` with perforation lines, waves, and parent obligations.
4. A one-child side quest returns `lightweight_delegate`, not one-child cleave.
5. Dirty tree/submodule ambiguity produces warnings and downgrades or checkpoint requirements.
6. The returned `decision` remains backward-compatible for existing callers.
7. The strategy includes enough data to produce a valid `cleave_run` plan when cleave is selected.
8. Parent-mediated artifact/message expectations are represented in the workstream event plan.

### First implementation slice

Recommended first code slice:

1. Define DTOs for `DecompositionStrategy`, `PerforationLine`, `ConfidenceBreakdown`, `AssessmentWarning`, and `AssessmentDossier`.
2. Extend `assess_directive` to return strategy fields while preserving current heuristic fields.
3. Add deterministic downgrade gates for explicit paths, shared write scopes, dirty state, and child count.
4. Add focused unit tests for the eight acceptance criteria above using synthetic dossiers.
5. Only after DTO/gate tests pass, add top-tier model adjudication over the dossier.


## Locked UX Direction — Agent-Initiated Cleave Gate, Menu Approval, Workbench Process Tree

### Decision

Cleave execution is **agent-initiated but operator-authorized**.

The normal path is not that the operator manually types a cleave command. The normal path is:

```text
operator gives a large directive
  -> agent decides a decomposition assessment is warranted
  -> agent calls cleave_assess
  -> harness produces a pending cleave approval object
  -> operator acts through a menu approval gate
  -> approved action starts cleave_run or an alternate execution path
```

Registry commands still exist, but they are action backends for TUI/menu/ACP/fallback slash surfaces. They are not the primary expected UX.

### Surface split

The menu and Workbench have different jobs.

#### Menu: authority and choice

The approval gate is a real menu. It owns operator choice and authority transfer.

The menu should expose actions such as:

```text
Review details
Approve and run
Modify plan
Deny
Run phased in parent
Save assessment
View evidence
Reassess
Help
```

The menu may be rendered by the TUI, command palette, ACP action list, or another semantic surface, but it is semantically an approval/action menu. It is not Workbench content pretending to be a menu.

The menu must be backed by shared action handlers, conceptually:

```text
cleave.approve(assessment_id)
cleave.modify(assessment_id, patch_or_instruction)
cleave.deny(assessment_id)
cleave.phased(assessment_id)
cleave.save(assessment_id)
cleave.evidence(assessment_id)
cleave.reassess(assessment_id)
```

These actions should route through the command/action registry so keyboard bindings, command palette, ACP, tests, and fallback slash invocation share semantics.

#### Workbench: active process tree

The Workbench is for operational state, not authority selection.

Before approval, Workbench may show a compact pending approval row so the operator knows the session is blocked:

```text
Pending approval: cleave_27 — sequential children, 3 waves, medium risk, high cost
```

But the Workbench should not become the approval menu. Selecting that row opens the approval menu/details surface.

After approval, Workbench becomes the live cleave process tree:

```text
cleave_27 running
  Wave 1
    event-schema           running
      workspace: ...
      last activity: cargo test -p omegon workstream
  Wave 2
    delegate-events        pending
    cleave-events          pending
  Wave 3
    workbench-projection   pending

Parent obligations
  merge wave 1
  validate after each wave
  synthesize final result
```

The Workbench should show process status, not ask for permission inline. Its responsibilities are:

- active/pending/completed child workstreams;
- wave structure;
- child status and recent activity;
- workspace/substrate identity;
- merge and validation state;
- failures, conflicts, blocked states;
- parent synthesis obligations.

### Approval-gate state machine

```text
assessment_requested
  -> assessment_ready
  -> approval_required
  -> approved | modified | denied | phased | saved | superseded
  -> running | parent_plan_active | closed
```

Only the operator approval/menu path, or a future explicit automation policy, may transition `approval_required -> running`.

### High-cost confirmation

For medium/low-risk plans, a single menu approval is enough. For high-cost or high-risk plans, approval should require a confirmation step:

```text
Approve and run 5 child agents across 4 waves? y/N
```

This protects against accidental single-key launches.

### Future automation

Automation should reuse the same approval object and action path. A future opt-in policy may synthesize an approval only when all configured limits pass, for example:

```text
auto_cleave = true
max_risk = low
max_children = 2
requires_clean_tree = true
allowed_models = [A-tier, S-tier]
```

Default behavior remains manual approval.

### Design principle

```text
model = judgment
operator = authority
harness = execution
menu = authority transfer
workbench = process visibility
```
