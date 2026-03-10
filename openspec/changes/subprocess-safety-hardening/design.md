# Subprocess safety hardening — Design

## Spec-Derived Architecture

### security/processes

- **Browser launch helpers avoid shell-string command construction** (added) — 2 scenarios
- **Ollama shutdown avoids broad pkill patterns** (added) — 2 scenarios
- **Shell-based helper execution is isolated behind reviewed wrappers** (added) — 2 scenarios

## Scope

Harden the first subprocess/process-management slice of repo consolidation by replacing shell-string browser launch behavior in the web UI, removing broad `pkill -f` shutdown behavior from local inference, and tightening bootstrap helper execution around explicit command/argv dispatch. This slice is intentionally limited to immediate process-safety risks and regression coverage; it does not attempt the larger architectural decomposition of every oversized extension yet.

## File Changes

- `extensions/web-ui/index.ts` — replace shell-string browser launch with explicit executable + argv dispatch
- `extensions/web-ui/index.test.ts` — assert browser-open behavior through the safer launch path
- `extensions/local-inference/index.ts` — replace broad Ollama shutdown behavior with managed-process termination or narrow fallback behavior
- `extensions/local-inference/index.test.ts` — cover managed shutdown and no-managed-child behavior
- `extensions/bootstrap/index.ts` — isolate helper execution behind explicit command/argv dispatch or constrained wrapper boundaries
- `extensions/bootstrap/*.test.ts` — add regression coverage for hardened helper execution behavior
- `docs/subprocess-safety-hardening.md` — keep design-tree implementation notes aligned with the delivered file scope
