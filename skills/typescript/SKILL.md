---
name: typescript
description: TypeScript development conventions. Covers strict typing, async patterns, error handling, Node.js API usage, and testing with node:test.
---

# TypeScript Development Skill

Conventions for TypeScript code in Omegon and related projects.

## Strict Typing

- **Never use `any`** as a type annotation. Use `unknown` and narrow, or define a proper interface.
- **Prefer `interface` over `type`** for object shapes (better error messages, extensibility).
- **Use `as const` assertions** for literal arrays used as union sources.
- **Import types separately**: `import type { Foo } from "./bar.js"` — keeps runtime imports clean.

```typescript
// ❌ Bad
const data: any = await resp.json();
function process(items: any[]) { ... }

// ✅ Good
interface ApiResponse { data: Model[]; total: number; }
const data: ApiResponse = await resp.json();
function process(items: readonly Model[]) { ... }
```

## Async Patterns

### Don't mark functions `async` unless they `await`

```typescript
// ❌ Unnecessarily async — wraps return value in an extra promise
const text = async (msg: string) => ({ content: msg });

// ✅ Sync function returns plain object
const text = (msg: string) => ({ content: msg });
```

### Don't `.then(r => r)` — it's a no-op identity transform

```typescript
// ❌ Pointless promise chain
resolve(text("done").then(r => r));

// ✅ Direct
resolve(text("done"));
```

### Avoid `new Promise()` when `async/await` suffices

```typescript
// ❌ Promise constructor anti-pattern
function doWork(): Promise<string> {
  return new Promise((resolve) => {
    someCallback((result) => resolve(result));
  });
}

// ✅ Use promisify or async/await when possible
import { promisify } from "node:util";
const doWork = promisify(someCallback);
```

### Exception: `new Promise()` is correct for wrapping event-emitter APIs

```typescript
// ✅ Correct use — child process exit is event-based, not promise-based
function runCommand(cmd: string): Promise<number> {
  return new Promise((resolve) => {
    const child = spawn("sh", ["-c", cmd]);
    child.on("exit", (code) => resolve(code ?? 1));
    child.on("error", () => resolve(1));
  });
}
```

## Error Handling

- **Never swallow errors silently** — at minimum log or return an error indicator.
- **Use `catch` with typed narrowing**, not `catch (e: any)`.
- **Prefer returning error objects** over throwing in tool handlers.

```typescript
// ❌ Swallowed error
try { await riskyOp(); } catch { }

// ✅ Intentional ignore with comment
try { await riskyOp(); } catch { /* expected: file may not exist */ }

// ✅ Error narrowing
try {
  await riskyOp();
} catch (err) {
  const message = err instanceof Error ? err.message : String(err);
  return { error: message };
}
```

## Node.js API Usage

### Use `node:` prefix for built-in modules

```typescript
// ❌
import { readFileSync } from "fs";

// ✅
import { readFileSync } from "node:fs";
```

### Use `execSync` only for quick checks, never for long-running operations

```typescript
// ✅ Quick binary check
function hasBinary(name: string): boolean {
  try { execSync(`which ${name}`, { stdio: "ignore" }); return true; }
  catch { return false; }
}

// ❌ Blocking install (hangs TUI for minutes)
execSync("cargo install my-tool");

// ✅ Async spawn for install
const child = spawn("cargo", ["install", "my-tool"], { stdio: "pipe" });
```

### Cache expensive checks that won't change mid-session

```typescript
// ❌ Spawns a shell on every call
function hasOllama(): boolean {
  try { execSync("which ollama", { stdio: "ignore" }); return true; }
  catch { return false; }
}

// ✅ Cached
let _hasOllama: boolean | null = null;
function hasOllama(): boolean {
  if (_hasOllama !== null) return _hasOllama;
  try { execSync("which ollama", { stdio: "ignore" }); _hasOllama = true; }
  catch { _hasOllama = false; }
  return _hasOllama;
}
```

## Testing with `node:test`

Omegon uses the built-in Node.js test runner. Run with: `npx tsx --test extensions/**/*.test.ts`

```typescript
import { describe, it } from "node:test";
import * as assert from "node:assert/strict";

describe("myModule", () => {
  it("does the thing", () => {
    assert.equal(myFunc(1), 2);
  });

  it("handles edge cases", () => {
    assert.throws(() => myFunc(-1), /must be positive/);
  });
});
```

### Test conventions

- **Test files**: `*.test.ts` co-located with source.
- **Import from source**, not compiled output — tsx handles compilation.
- **Test behavior, not implementation** — assert on public API results, not internal state.
- **Include negative tests** — error paths, invalid inputs, boundary values.
- **No mocking frameworks** — use dependency injection or simple stubs.

## Module Conventions

- **Use `.js` extensions in imports** — TypeScript requires this for ESM resolution even though source files are `.ts`.
- **Export `default function`** for extension entry points.
- **Named exports** for utility modules (types, helpers).
- **No barrel files** (`index.ts` that re-exports everything) — import directly from the source module.

```typescript
// ❌ Bare specifier
import { checkAll } from "./deps";

// ✅ With .js extension
import { checkAll } from "./deps.js";
```

## Code Organization

- **Single responsibility**: one concept per file. Split when a file exceeds ~400 lines.
- **Types first**: define interfaces at the top or in a `types.ts` file.
- **Pure functions in separate modules**: domain logic (`auth.ts`, `spec.ts`) separate from extension wiring (`index.ts`).
- **Avoid God objects**: don't accumulate state in a single closure. Extract helper functions.

## Type Checking

Projects using runtime transpilation (jiti, tsx, esbuild, etc.) **must** run `tsc --noEmit` as a separate type-checking gate. Runtime transpilers strip types without checking them — type errors compile and run but silently produce incorrect behavior.

```bash
# Add to package.json scripts
"typecheck": "tsc --noEmit"
"check": "tsc --noEmit && npm test"
```

### The Shadow Interface Anti-Pattern

**Never redefine types that exist in an upstream SDK.** When you copy-paste an interface instead of importing it, your local "shadow" drifts from the real type as the SDK evolves. The compiler can't catch the mismatch because it doesn't know they're supposed to be the same type.

```typescript
// ❌ Shadow interface — drifts silently
interface ToolResult {
  content: { type: string; text: string }[];
}

// ✅ Import from SDK
import type { AgentToolResult } from "@styrene-lab/pi-coding-agent";
```

**Directive:** Always import SDK types. Never redefine them locally. If an SDK type is not exported, file an issue or use module augmentation — don't copy the shape.
