---
id: extension-type-safety
title: Extension Type Safety — Preventing API Hallucinations
status: implemented
tags: [dx, quality, guardrails]
open_questions: []
branches: ["feature/extension-type-safety"]
openspec_change: extension-type-safety
---

# Extension Type Safety — Preventing API Hallucinations

## Overview

A non-existent method (ctx.say) survived authoring, review, testing, and deployment into the bootstrap extension. Root cause analysis and guardrail design to prevent phantom API usage from reaching runtime.

## Research

### Kill Chain — How ctx.say Survived 5 Layers

**Layer 1: Authoring** — AI generated `ctx.say()` which doesn't exist on `ExtensionCommandContext`. The SDK types define `ctx.ui.notify()`. No static analysis ran at write time.

**Layer 2: No tsconfig.json** — pi-kit has NO TypeScript compiler dependency and NO tsconfig. Extensions are transpiled at runtime by jiti, which strips types without checking them. There is literally no type-checking step anywhere in the pipeline.

**Layer 3: Test coverage gap** — `extensions/bootstrap/deps.test.ts` tests the dep-checking logic (pure functions) but never exercises the command handlers that call `ctx.say`. No integration test ever constructs a real or mock `ExtensionCommandContext` and calls the `/refresh` handler.

**Layer 4: Code review** — `/assess` reviews code but without type information, `ctx.say` looks plausible. The reviewer (AI) may even "know" about `ctx.say` from training data on other frameworks.

**Layer 5: Runtime** — jiti transpiles TS→JS by erasing types. JavaScript is happy to access `.say` on any object — it's just `undefined` until you call it. The error only surfaces when someone actually runs `/refresh`.

### Available Guardrails — Ordered by Catch-Earliness

**G1: tsconfig.json + `tsc --noEmit` in test runner** (catches at authoring)
- Add `typescript` as devDependency, create tsconfig.json pointing at extensions/
- Add a `typecheck` npm script: `tsc --noEmit`
- Run before tests or as a pre-commit hook
- Cost: ~2s on cold, <1s incremental. Zero runtime impact.
- This alone would have caught ctx.say immediately.

**G2: Extension API surface test** (catches at test time)
- A test that imports all extension entry points via jiti and asserts exported handlers are callable against a strict mock of ExtensionCommandContext built from the actual type definition.
- The mock would use `Proxy` to throw on any property access not in the interface.
- Catches runtime-only hallucinations even without tsc.

**G3: Strict context proxy in dev mode** (catches at runtime, dev only)
- pi's extension runner could wrap ExtensionCommandContext in a Proxy that throws on unknown property access when `PI_DEV=1` or `--strict-extensions`.
- This is the safety net for dynamic property access patterns that tsc can't catch.

**G4: /assess API validation pass** (catches at review)
- Teach the assessment skill to cross-reference method calls on `ctx`, `ctx.ui`, etc. against the SDK type definitions.
- The reviewer would `grep` for `ctx.` usage and verify each method exists in the `.d.ts`.

**G5: Extension smoke test harness** (catches at test time)
- A generic test that loads every registered extension, invokes every registered command with a Proxy-based mock context, and asserts no "X is not a function" errors.
- Can be generated automatically from package.json command registrations.

### tsc --noEmit Results with Bundler Resolution

With `"module": "ESNext"`, `"moduleResolution": "Bundler"`, `"allowImportingTsExtensions": true`, and `"strict": true`:

**154 errors total** across source + test files. Breakdown:
- **TS2345 (61)**: Argument type mismatches — mostly test mocks missing new required fields (specDomains, skills, implementingCount, etc.) and `"success"` not in notify's type union
- **TS2322 (24)**: Type assignment mismatches — similar pattern, test data incomplete
- **TS2339 (21)**: Property doesn't exist — real bugs like accessing `.text` on a union, `.appendEntry` wrong context type, `.isError` not on result type
- **TS2554 (19)**: Wrong argument count — `ctx.ui.notify()` called with 1 arg where SDK requires 2 (the `type` param isn't optional in the .d.ts)
- **TS18048 (19)**: Possibly undefined — `ctx.ui` nullability, `change` possibly undefined
- **TS7015 (3)**: Element implicitly has any — index signature issues
- **TS2304/2300/2741/2307 (6)**: Misc — duplicate identifiers, missing modules, missing properties

**Non-test errors: ~121** (production code). Test-only errors: ~33 (incomplete mock data).

The TS2554 errors (notify called with 1 arg) reveal the SDK declares `notify(message: string, type: "info" | "warning" | "error")` — the `type` parameter is NOT optional. Every single-arg notify call is technically wrong. Also `"success"` is not in the union — it's used in defaults.ts and local-inference but the SDK doesn't accept it.

### Deeper Root Cause — Shadow Interface Pattern

The bootstrap extension defines its own `CommandContext` interface:
```typescript
interface CommandContext {
  say: (msg: string) => void;
  hasUI?: boolean;
  ui?: {
    notify: (msg: string, level: string) => void;
    confirm: (title: string, message: string) => Promise<boolean>;
  };
}
```

This is a **shadow type** — a hand-rolled interface that duplicates (incorrectly) the SDK's `ExtensionCommandContext`. The `say` method was never a hallucinated call on a real type — it was a hallucinated *interface definition* that legitimized the hallucinated method.

This means even `tsc --noEmit` alone won't catch this class of bug, because the file is internally type-consistent. The shadow interface types `say` as existing, so calling `say` passes the type checker.

**Additional guardrail needed**: A lint rule or skill directive that prohibits defining custom interfaces that shadow SDK types. Extensions must import and use `ExtensionCommandContext`, `ExtensionContext`, `ExtensionAPI` from `@mariozechner/pi-coding-agent` — never redefine their shapes.

The fix: delete the shadow `CommandContext` interface, import the real SDK types, and adapt the code to match the actual API.

## Decisions

### Decision: Use tsc --noEmit with Bundler moduleResolution as the typecheck gate

**Status:** decided
**Rationale:** Bundler resolution matches jiti's actual behavior (allows .ts extensions, ESM imports). skipLibCheck avoids SDK-internal type issues. strict:true catches the class of bugs we care about. 154 errors is a tractable backlog — most are pattern-based. Zero runtime cost.

### Decision: Separate typecheck script, run alongside tests not as pre-commit gate

**Status:** decided
**Rationale:** Pre-commit hooks are easily bypassed and annoying during WIP commits. Better: `npm run typecheck` as a parallel CI gate and as a directive that the agent must run after modifying .ts files. The test command stays fast (tsx --test). A combined `npm run check` runs both.

### Decision: Fix all 154 errors to establish green baseline before mandating typecheck

**Status:** decided
**Rationale:** A typecheck gate is useless if it's already red — nobody will respect it. Must get to 0 errors first, then the directive "tsc must stay green" has teeth. Errors are pattern-based and cleavable.

### Decision: Update TypeScript skill with type-checking mandate for plugin-loaded codebases

**Status:** decided
**Rationale:** The skill should have flagged that jiti-transpiled code with no tsc step is a ticking bomb. Add a section on "Runtime-only TypeScript" anti-pattern — if there's no build step, you MUST have a typecheck step, because the runtime won't catch type errors until the code path executes. Also add project-specific directive to AGENTS.md.

## Open Questions

*No open questions.*
