# Deterministic Guardrail Integration — Baking Static Analysis into the Feature Lifecycle — Design

## Architecture Decisions

### Decision: Advisory with escalation — guardrails inject output and flag prominently but don't hard-block

**Status:** decided
**Rationale:** Hard-blocking on test flakes would halt all development. Advisory with escalation means: (1) guardrail output is always injected into prompts/reports, (2) failures are flagged as Critical-tier issues, (3) review loop treats deterministic failures as severity:critical. Can revisit if advisory proves too soft.

### Decision: Skill-associated guardrails with condition-based activation

**Status:** decided
**Rationale:** Guardrails are declared per-skill in SKILL.md frontmatter with a `condition` field (e.g., file_exists(tsconfig.json)). The cleave skill-matching system already resolves skills per child — extend it to also collect applicable guardrails. Organic discovery: guardrails activate when the project creates the config file they depend on. No separate registry file needed — skills ARE the registry. Also check package.json scripts as a fallback: if "typecheck" or "lint" scripts exist, auto-discover them even without skill-level declaration.

### Decision: Hardcode now — implement I1, I2, I3, I4 for tsc+tests immediately

**Status:** decided
**Rationale:** User directive: "BALLS TO THE WALL — don't talk about it, BE ABOUT IT." Implement the 4 highest-impact integration points now with hardcoded tsc+test detection. Generalize the skill-frontmatter guardrail format as part of the same change. Session-start health check (I6) also included as low-effort high-visibility.

### Decision: Augment review — deterministic output feeds into opus review, doesn't replace it

**Status:** decided
**Rationale:** tsc catches type errors, opus catches logic errors — they're complementary. Deterministic findings are prepended to the review prompt as "Confirmed issues (deterministic — not opinions)" section. If guardrails pass cleanly, the review prompt says "All deterministic checks passed" which still has value — it tells the reviewer to focus on logic, not types.

## Research Context

### Feature Lifecycle Phases and Current Guardrail Coverage

The pi-kit feature lifecycle has 7 distinct phases where code is produced or evaluated. Currently, deterministic checks (tsc, tests) exist but are **never automatically invoked** — they depend entirely on the agent remembering to run them.

| Phase | What happens | Current guardrails | Gap |
|-------|-------------|-------------------|-----|
| **1. Authoring** | Agent writes/edits .ts files | AGENTS.md directive (advisory) | No automatic check after file edit |
| **2. Cleave child execution** | Child agent writes code in worktree | Task file says "run tests" (advisory) | No automatic tsc/test gate before child reports success |
| **3. Cleave review loop** | Opus reviews child output (review.ts) | Adversarial prompt (heuristic) | Reviewer has no tsc output — reviews code by reading, not by running the compiler |
| **4. Cleave merge** | Branches merged to base | None | No post-merge tsc/test run — merged code may have type errors from cross-child interactions |
| **5. `/assess cleave`** | Adversarial review + auto-fix | Git diff analysis (heuristic) | No tsc output in the review prompt |
| **6. `/assess diff`** | Review-only analysis | Git diff analysis (heuristic) | Same gap |
| **7. `/assess spec`** | OpenSpec scenario validation | Scenario matching (heuristic) | No deterministic checks |

**Key insight**: Every "guardrail" today is either advisory (directive text) or heuristic (AI reviews code). The only deterministic oracles (tsc, test runner) are never wired into any automated gate. They exist as tools the agent *can* invoke but nothing *forces* invocation.

### Proposed Integration Architecture — Guardrail Registry + Automatic Gates

**Core concept: A project-level guardrail registry** — a declarative config (in package.json or a dedicated file) that lists deterministic checks the project requires. The pi-kit infrastructure then invokes these checks automatically at the right lifecycle points.

```jsonc
// package.json or .pi/guardrails.json
{
  "guardrails": {
    "checks": [
      { "name": "typecheck", "cmd": "npx tsc --noEmit", "timeout": 30 },
      { "name": "test", "cmd": "npx tsx --test extensions/*.test.ts extensions/**/*.test.ts", "timeout": 120 },
      { "name": "lint", "cmd": "npx eslint extensions/", "timeout": 30 }
    ],
    "gates": {
      "child_completion": ["typecheck"],
      "post_merge": ["typecheck", "test"],
      "assess": ["typecheck"],
      "pre_commit": ["typecheck"]
    }
  }
}
```

**Integration points (7 concrete changes):**

**I1: Cleave child task file injection** — `generateTaskFile()` in workspace.ts currently includes skill directives. Add a "Project Guardrails" section that tells the child: "Before reporting success, run these commands and include their output in the Verification section. If any fail, fix the errors before completing." This makes the child agent self-enforce.

**I2: Cleave review loop — deterministic pre-check** — In `executeWithReview()` (review.ts), before spawning the opus reviewer, run the guardrail checks programmatically. If tsc fails, inject the error output into the review prompt so the reviewer has deterministic evidence, not just vibes. This is the "give the reviewer a microscope" pattern.

**I3: Cleave post-merge verification** — In the merge phase (index.ts ~L1250), after all branches merge, run the full guardrail suite. If typecheck or tests fail post-merge, the report should flag it as a merge-introduced regression and include the error output. Currently the report just says "merged successfully" based on git merge exit code.

**I4: `/assess` commands — tsc preamble** — For `/assess cleave` and `/assess diff`, run `tsc --noEmit` before generating the review prompt. Include any errors in the prompt as "Deterministic findings — these are confirmed bugs, not opinions." This gives the AI reviewer ground truth to work from.

**I5: `/assess spec` — run guardrails alongside scenario matching** — After matching OpenSpec scenarios to implementation, run the guardrail checks. Report them as a separate "Static Analysis" section before the scenario results.

**I6: Session-start health check** — On `session_start`, run the guardrails in background. If any fail, surface a notification: "⚠ typecheck has 3 errors — run `/assess cleave` or fix manually." This catches drift between sessions.

**I7: Commit-time directive reinforcement** — When the agent is about to commit (detected by the git skill or a pre-commit pattern), the skill directive should explicitly say "run `npm run typecheck` and include the output." This is the last line of defense.

**Why registry, not hardcoded?** Different projects have different checks — a Python project needs mypy+ruff, a Rust project needs clippy+cargo test. The registry pattern means the guardrail infrastructure works for any pi-kit project, not just this one.

### Implementation Effort and Priority Ordering

Ordered by impact-to-effort ratio:

**Tier 1 — High impact, low effort (do first)**
- **I1: Child task file injection** — 20 lines in workspace.ts. Reads guardrail config, appends a "Guardrails" section to task markdown. Children self-enforce. Catches errors before they're ever committed to a branch.
- **I2: Review loop pre-check** — 30 lines in review.ts. Run checks before spawning reviewer. Inject output into prompt. Reviewer now has ground truth.
- **I4: `/assess` preamble** — 20 lines in index.ts assess handler. Run tsc, prepend output to review prompt.

**Tier 2 — High impact, moderate effort**
- **I3: Post-merge verification** — 40 lines in the merge section of index.ts. Run checks after merge, include in report. The report format needs a new "Static Analysis" section.
- **I6: Session-start health check** — 30 lines in a session_start handler. Run checks in background, notify on failure. Need to be careful not to slow startup.

**Tier 3 — Ecosystem value, higher effort**
- **Guardrail registry format** — Design the config schema, implement loader, make it discoverable from package.json. This makes the whole system project-agnostic.
- **I5: `/assess spec` integration** — Moderate coupling with the OpenSpec verification flow.
- **I7: Commit-time reinforcement** — Git skill update, detection of commit intent.

**The critical insight**: I1 and I2 together mean that no cleave child can report success without passing the type checker. Since cleave is how most multi-file changes are implemented, this covers the highest-volume path.

### Existing Infrastructure for Organic Discovery

The skill-matching and profile-detection systems already have the patterns we need:

**Skill matching (cleave/skills.ts)**: File scope → skill name mapping. `*.ts` → `typescript`, `*.py` → `python`, etc. Skills are resolved to SKILL.md paths and injected into child task files. This is the natural place to also resolve guardrails.

**Profile detection (tool-profile/profiles.ts)**: `detectProfiles(cwd)` checks for `.git`, `package.json` with pi config, `ollama` binary, etc. Returns which tool profiles are active. This runs at session start.

**The organic discovery path for a new project would be:**
1. User starts working in a new directory
2. Profile detection identifies it as a coding project (`.git` exists)
3. As design discussions approach implementation, they create a `tsconfig.json` or `pyproject.toml`
4. Next cleave dispatch detects `*.ts` scope → matches `typescript` skill
5. The typescript skill already says "run `tsc --noEmit`" — but currently only as prose

**The gap**: Skills inject instructions (read SKILL.md) but don't inject executable guardrails. The skill says "you should typecheck" but nothing programmatically runs `tsc` or injects its output.

**Proposal: Skill-associated guardrails** — extend the SkillMapping or SKILL.md format to declare guardrail commands:

```typescript
// In skills.ts DEFAULT_MAPPINGS
{
  patterns: ["*.ts", "tsconfig.json"],
  skill: "typescript",
  guardrails: [
    { name: "typecheck", cmd: "npx tsc --noEmit", timeout: 30 }
  ]
}
```

Or via a frontmatter field in SKILL.md:
```yaml
---
name: typescript
guardrails:
  - name: typecheck
    cmd: npx tsc --noEmit
    timeout: 30
    condition: file_exists(tsconfig.json)
---
```

The `condition: file_exists(tsconfig.json)` means the guardrail only activates when the project has actually set up type checking — a brand new directory wouldn't get it until the design phase reaches the point of creating tsconfig. This is the organic discovery the user described.

**The lifecycle**: design discussion → "we need type safety" decision → create tsconfig.json → next cleave dispatch auto-detects the guardrail → children are forced to pass it. No manual configuration needed.

## File Changes

- `extensions/cleave/guardrails.ts` (new) — Guardrail discovery, execution, and result formatting. Discovers checks from package.json scripts (typecheck, lint, test) and skill frontmatter. Runs checks via child_process.execSync with timeout. Formats output for injection into prompts/reports.
- `extensions/cleave/workspace.ts` (modified) — I1: generateTaskFile() calls guardrail discovery, appends 'Project Guardrails' section with executable commands and exit-code requirement.
- `extensions/cleave/review.ts` (modified) — I2: executeWithReview() runs guardrail checks before spawning opus reviewer. Injects output into review prompt as 'Deterministic Findings' section.
- `extensions/cleave/index.ts` (modified) — I3: Post-merge phase runs guardrail suite, includes results in report. I4: /assess cleave and /assess diff run guardrails, prepend to review prompt.
- `extensions/dashboard/index.ts` (modified) — I6: session_start handler runs guardrail checks in background, surfaces notification on failure.
- `skills/typescript/SKILL.md` (modified) — Add guardrails frontmatter: typecheck with condition file_exists(tsconfig.json).
- `skills/python/SKILL.md` (modified) — Add guardrails frontmatter: typecheck (mypy) with condition file_exists(pyproject.toml), lint (ruff) with condition.
- `skills/rust/SKILL.md` (modified) — Add guardrails frontmatter: clippy with condition file_exists(Cargo.toml), test with condition.

## Constraints

- Guardrail execution must not block the main thread — use execSync with timeout cap (30s default)
- Guardrail output injected into prompts must be capped (e.g., first 50 lines of tsc errors) to avoid blowing context
- Discovery must be fast (<100ms) — read package.json scripts + check file existence, no spawning processes for detection
- Existing 937 tests must continue passing — guardrail integration is additive
- as-any count must not increase beyond current 56 baseline
