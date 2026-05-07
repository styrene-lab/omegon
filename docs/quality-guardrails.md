+++
id = "06462e8d-f35a-4e3b-bf0a-384915e91136"
kind = "document"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
design_docs = ["design/deterministic-guardrails.md", "design/extension-type-safety.md"]
last_updated = "2026-03-10"
openspec_baselines = []
subsystem = "quality-guardrails"
+++

# Quality & Guardrails

> Static analysis integration, type safety enforcement, and deterministic quality checks baked into the feature lifecycle.

## What It Does

Quality guardrails provide automated checks that run during development to catch issues before they reach review:

1. **TypeScript strict mode**: `npx tsc --noEmit` enforced before commits. `npm run check` runs both typecheck and test suite.
2. **Extension type safety**: Extensions import types from `@styrene-lab/pi-coding-agent` — the public API surface. No internal imports allowed.
3. **Dependency probing**: Bootstrap checks for required external tools (d2, pandoc, pdftoppm, Ollama, clipboard commands) and warns on missing dependencies.
4. **Test suite**: 1298+ tests via `node:test` runner, covering all extensions.

## Key Files

| File | Role |
|------|------|
| `extensions/bootstrap/deps.ts` | Runtime dependency checks — probes for d2, pandoc, pdftoppm, Ollama, clipboard tools |
| `extensions/bootstrap/index.ts` | First-run setup, dependency warning display |
| `tsconfig.json` | TypeScript strict configuration |
| `package.json` | `check`, `typecheck`, `test` scripts |

## Design Decisions

- **Bake static analysis into lifecycle**: `npm run check` (typecheck + tests) runs before any commit. CI enforces on push/PR.
- **Extension types from public API only**: All `@styrene-lab/pi-ai` imports eliminated — `StringEnum` inlined to `extensions/lib/typebox-helpers.ts`. Runtime imports limited to public exports.
- **Dependency probing at startup, not install**: Bootstrap checks what's available on the system at first run, not during npm install. Warnings are informational, not blocking.

## Constraints & Known Limitations

- No linter (eslint/biome) configured — relies on TypeScript strict mode and test coverage
- Dependency probing is macOS-focused (pbpaste, system_profiler)
- CI runs on Node 20+22 only

## Related Subsystems

- [Operator Profile](operator-profile.md) — capability probing for provider and tool availability
- [Cleave](cleave.md) — guardrails run in child processes during review loop
