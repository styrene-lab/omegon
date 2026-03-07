# Design Tree Lifecycle: Implementation Tracking + Branch Binding — Design

## Architecture Decisions

### Decision: D1: Branch naming uses conventional prefix + node ID, with override

**Status:** decided
**Rationale:** Default branch name follows git skill conventions: `feature/<node-id>`, `fix/<node-id>`, `refactor/<node-id>`, etc. The prefix is chosen based on context (implement action defaults to `feature/`). An optional `branch` frontmatter field allows explicit override. This aligns with the existing conventional commit skill patterns.

### Decision: D2: Auto-transition to implementing on scaffold

**Status:** decided
**Rationale:** The `implement` action already gates on `status === "decided"`. Scaffolding the OpenSpec change and transitioning to `implementing` in a single atomic operation is the natural behavior — no reason for a separate manual step.

### Decision: D4: Frontmatter gets both branch and openspec_change fields

**Status:** decided
**Rationale:** Both fields are cheap and create full traceability: node ↔ branch ↔ openspec_change. The `implement` action writes both automatically since it knows the OpenSpec change name at scaffold time.

### Decision: D5: Dashboard shows implementing with ⚙ icon and branch name

**Status:** decided
**Rationale:** Compact: `◈ D:1 I:1 /2`. Raised: `implementing` gets `⚙` with accent color (active work), vs `●` success for decided. Raised mode shows branch name inline: `⚙ skill-aware-dispatch → feature/skill-aware-dispatch`.

### Decision: D3: Convention-based auto-association + OpenSpec archive gate

**Status:** decided
**Rationale:** Branch association is ambient — zero user input. On branch creation, the design-tree extension checks if the branch name contains a known implementing node ID as a segment (longest match wins). Auto-appends to that node's `branches[]` history with a log line, no prompt. The acceptance gate is OpenSpec archive (Gate A) — `/opsx:archive` requires `/assess spec` to pass, then flips the node to `implemented`. This keeps the user in natural flow without CLI bookkeeping friction.

## Research Context

### Multi-branch implementing lifecycle

An `implementing` node may require multiple branches before reaching `implemented`. The primary branch (feature/) is created on scaffold. If assessment fails post-merge, fix branches are spawned and associated with the same node. The node accumulates a `branches: string[]` history in frontmatter.

Three acceptance gate options:
- **Gate A: OpenSpec archive** — already requires /assess spec pass. Cleanest existing checkpoint.
- **Gate B: Explicit /assess spec pass** — looser, but no ceremony trigger for `implemented`.
- **Gate C: All branches merged** — git-level only, no correctness check.

Branch association options:
- **Explicit**: command like `/design associate-branch <branch-name>`
- **Convention-based**: auto-detect branches containing node ID as segment
- **Prompted**: agent notices matching branch name and suggests association

## File Changes

- `extensions/design-tree/types.ts` (modified) — Add implementing/implemented to NodeStatus, STATUS_ICONS, STATUS_COLORS. Add branches[] and openspec_change to DesignNode.
- `extensions/design-tree/tree.ts` (modified) — Parse/serialize new frontmatter fields (branches, openspec_change). Update scaffoldOpenSpecChange to set status=implementing and write branch+openspec_change fields.
- `extensions/design-tree/index.ts` (modified) — Add branch auto-detection hook (onBranchChange or session event). Update implement action to create git branch and set status. Wire OpenSpec archive event to set implemented.
- `extensions/design-tree/tree.test.ts` (modified) — Tests for new statuses, frontmatter parsing, branch association logic.
- `extensions/dashboard/types.ts` (modified) — Add implementingCount to DesignTreeDashboardState.
- `extensions/dashboard/footer.ts` (modified) — Render implementing count in compact mode, branch names in raised mode.
- `extensions/openspec/index.ts` (modified) — On archive, find implementing node with matching openspec_change and set status to implemented.

## Constraints

- Branch auto-detection must use segment matching (split on / and -) to avoid false positives between node IDs that are substrings of each other — longest match wins
- No user prompts or CLI commands for branch association — fully ambient
- implementing status icon ⚙ with accent color, implemented gets ✓ with success color
