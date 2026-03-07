# Deterministic Guardrail Integration — Tasks

## 1. Core guardrail engine
<!-- @spec-domains: guardrails -->
<!-- @skills: typescript, pi-extensions -->

- [ ] 1.1 Create extensions/cleave/guardrails.ts — GuardrailCheck type, discoverGuardrails(cwd), runGuardrails(cwd, checks), formatGuardrailOutput()
- [ ] 1.2 Discovery: parse package.json scripts for "typecheck"/"lint" keys, check file_exists conditions (tsconfig.json, pyproject.toml, Cargo.toml)
- [ ] 1.3 Discovery: parse SKILL.md frontmatter `guardrails:` field for skill-declared checks
- [ ] 1.4 Execution: execSync with configurable timeout, capture stdout+stderr, exit code
- [ ] 1.5 Formatting: cap output at 50 lines, format as markdown for prompt injection
- [ ] 1.6 Add guardrails.test.ts — discovery from mock package.json, condition evaluation, output capping

## 2. I1 — Child task file guardrail injection
<!-- @spec-domains: child-injection -->
<!-- @skills: typescript, pi-extensions -->

- [ ] 2.1 Add buildGuardrailSection() to workspace.ts — generates "Project Guardrails" markdown section
- [ ] 2.2 Wire into generateTaskFile() — call discoverGuardrails, append section after skill section
- [ ] 2.3 Section format: numbered commands with "run these before reporting success, fix failures before completing"
- [ ] 2.4 Add tests in workspace.test.ts for guardrail section generation

## 3. I2 — Review loop deterministic pre-check
<!-- @spec-domains: review-precheck -->
<!-- @skills: typescript, pi-extensions -->

- [ ] 3.1 Add runPreReviewChecks() to review.ts — runs guardrails in the child's worktree cwd
- [ ] 3.2 Inject output into review prompt as "## Deterministic Findings" section before the review instructions
- [ ] 3.3 If all checks pass, inject "All deterministic checks passed — focus on logic and architecture"
- [ ] 3.4 If checks fail, inject error output prefixed with "These are confirmed bugs (compiler output, not opinions):"

## 4. I3 + I4 — Post-merge verification and /assess preamble
<!-- @spec-domains: merge-assess -->
<!-- @skills: typescript, pi-extensions -->

- [ ] 4.1 Post-merge: after all branches merge successfully in cleave_run, run guardrail suite on merged codebase
- [ ] 4.2 Include guardrail results in cleave report as "### Static Analysis" section
- [ ] 4.3 /assess cleave: run guardrails before building review prompt, prepend as "Deterministic findings"
- [ ] 4.4 /assess diff: same preamble injection
- [ ] 4.5 Flag guardrail failures as Critical-severity in the review prompt framing

## 5. I6 — Session-start health check
<!-- @spec-domains: session-health -->
<!-- @skills: typescript, pi-extensions -->

- [ ] 5.1 In dashboard extension session_start handler, run discoverGuardrails + runGuardrails asynchronously
- [ ] 5.2 On failure: ctx.ui.notify with warning level, showing check name + error count
- [ ] 5.3 On success: silent (no notification clutter)
- [ ] 5.4 Timeout cap at 15s total — session start must not be noticeably delayed

## 6. Skill frontmatter guardrails
<!-- @spec-domains: skill-frontmatter -->
<!-- @skills: typescript -->

- [ ] 6.1 Update skills/typescript/SKILL.md frontmatter with guardrails field
- [ ] 6.2 Update skills/python/SKILL.md frontmatter with guardrails field
- [ ] 6.3 Update skills/rust/SKILL.md frontmatter with guardrails field
- [ ] 6.4 Add SKILL.md frontmatter parsing to guardrails.ts — read yaml frontmatter, extract guardrails array
- [ ] 6.5 Wire skill-discovered guardrails into discoverGuardrails() alongside package.json discovery

## 7. Verification

- [ ] 7.1 npx tsc --noEmit exits 0
- [ ] 7.2 All 937+ existing tests pass
- [ ] 7.3 New guardrails.test.ts tests pass
- [ ] 7.4 as-any count ≤ 56
