+++
id = "f87d7bff-5b78-4165-84b6-8bdc4f2d54cd"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Extension Type Safety — Tasks

## 1. Infrastructure — tsconfig + scripts + package.json
<!-- @spec-domains: infra -->

- [x] 1.1 Add typescript as devDependency (already done)
- [x] 1.2 Create tsconfig.json with Bundler moduleResolution (already done)
- [ ] 1.3 Add npm scripts: `typecheck` (`tsc --noEmit`), `check` (typecheck + test)
- [ ] 1.4 Add `"type": "module"` to package.json if needed for module resolution

## 2. Fix type errors — high-error files (bootstrap, project-memory, design-tree)
<!-- @spec-domains: type-fixes -->

- [ ] 2.1 Fix extensions/bootstrap/index.ts (35 errors) — notify arg count, ctx.ui nullability
- [ ] 2.2 Fix extensions/project-memory/index.ts (30 errors) — unknown params, property access
- [ ] 2.3 Fix extensions/design-tree/index.ts (20 errors) — unknown params, property access

## 3. Fix type errors — medium-error files (tool-profile, openspec, local-inference, cleave)
<!-- @spec-domains: type-fixes -->

- [ ] 3.1 Fix extensions/tool-profile/index.ts (6 errors)
- [ ] 3.2 Fix extensions/openspec/index.ts (5 errors)
- [ ] 3.3 Fix extensions/local-inference/index.ts (5 errors)
- [ ] 3.4 Fix extensions/cleave/index.ts (3 errors)
- [ ] 3.5 Fix extensions/model-budget.ts (3 errors)
- [ ] 3.6 Fix extensions/render/excalidraw/elements.ts (3 errors)
- [ ] 3.7 Fix extensions/project-memory/factstore.ts (3 errors)
- [ ] 3.8 Fix extensions/01-auth/index.ts (3 errors)

## 4. Fix type errors — low-error files (2 or fewer)
<!-- @spec-domains: type-fixes -->

- [ ] 4.1 Fix extensions/defaults.ts (2 errors) — "success" not in notify type union
- [ ] 4.2 Fix extensions/dashboard/index.ts (1 error) — appendEntry wrong context
- [ ] 4.3 Fix extensions/version-check.ts (1 error)
- [ ] 4.4 Fix extensions/offline-driver.ts (1 error)

## 5. Fix type errors — test files
<!-- @spec-domains: type-fixes -->

- [ ] 5.1 Fix extensions/dashboard/overlay-data.test.ts (14 errors) — missing required fields in mocks
- [ ] 5.2 Fix extensions/cleave/openspec.test.ts (13 errors) — TaskGroup mock missing specDomains/skills
- [ ] 5.3 Fix extensions/design-tree/tree.test.ts (5 errors)
- [ ] 5.4 Fix extensions/cleave/workspace.test.ts (1 error) — ChildPlan skills optionality

## 6. Skill + directive updates
<!-- @spec-domains: docs -->

- [ ] 6.1 Update skills/typescript/SKILL.md — add "Runtime-only TypeScript" anti-pattern section
- [ ] 6.2 Update AGENTS.md — add typecheck directive for all TS modifications
- [ ] 6.3 Update CONTRIBUTING.md — add typecheck to contributor workflow

## 7. Verification

- [ ] 7.1 `npx tsc --noEmit` exits 0
- [ ] 7.2 All 937+ tests still pass
- [ ] 7.3 No new `as any` escape hatches (grep for count, baseline against current)
