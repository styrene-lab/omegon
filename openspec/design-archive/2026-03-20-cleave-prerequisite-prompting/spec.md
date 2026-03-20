# Cleave prerequisite prompting — project context and finalization instructions — Design Spec (extracted)

> Auto-extracted from docs/cleave-prerequisite-prompting.md at decide-time.

## Decisions

### Three-layer enrichment: structural auto-discovery + convention sampling + explicit finalization contract (exploring)

Each layer addresses a different failure class. Structural context prevents wrong-path errors (submodule detection, dependency versions). Convention sampling prevents API misuse (show a real test, not just say 'write tests'). The finalization contract prevents uncommitted work and untested code. The layers are additive — each can be implemented and tested independently. Structural discovery runs once per cleave (shared across children). Convention sampling is per-child (scope-specific). Finalization is a static template expanded with project-specific paths.

### All three sources — Cargo.toml versions, code samples, and memory facts — each solving distinct failure classes (decided)

Dependency versions from Cargo.toml prevent API version mismatches (mockito 0.x vs 1.x). Code samples from existing tests prevent convention drift. Memory facts provide project-specific knowledge that neither source captures. The 4K token budget is sufficient for all three when extracted surgically — dep sections are ~20 lines, one test example is ~30 lines, 3-5 facts are ~15 lines. Memory recall is best-effort (skip if unavailable in child mode).

## Research Summary

### Failure modes from vault-secret-backend cleave

Three distinct failures caused by missing prerequisite context:\n\n1. **Wrong crate path**: Child created files at `crates/omegon-secrets/` instead of `core/crates/omegon-secrets/`. The scope paths in the task file were correct (after our fix), but the child's first attempt (before the fix) had stale paths. Even after fixing, the child didn't understand the submodule boundary.\n\n2. **Wrong library API**: Child used `mockito::mock()` (0.x API) instead of `mockito::Server::new_async()` (1.x API).…

### Proposed task file enrichment layers

The `build_task_file` function in orchestrator.rs should be extended with several new sections. These fall into three categories:\n\n### 1. Structural context (auto-discovered per project)\n- **Repo layout**: Is there a submodule? What's the actual working directory? Run `git submodule status` and include the result.\n- **Existing file contents**: For each file in scope that already exists, include a brief signature summary (public API, struct definitions, function signatures). The child needs t…
