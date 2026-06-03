---
title: Resource Open Real Backends #125 Workstream Handoff
status: active
tags: [handoff, 0.27.0, resource-open, host-actions]
---

# Resource Open Real Backends #125 Workstream Handoff

## Current branch state

Active branch:

```text
workstream/resource-open-real-backends-125
```

Working tree should be clean at handoff.

Latest branch commit:

```text
e79dc531 docs(design): track resource open backend follow-up
```

Branch ancestry:

```text
main
  └─ workstream/0.27-sdk-contract
       └─ workstream/resource-open-real-backends-125
```

This branch depends on the unmerged `workstream/0.27-sdk-contract` substrate.

## Parent workstream status

Parent branch:

```text
workstream/0.27-sdk-contract
```

Completed work:

- #124 — runtime extension SDK contract compatibility enforcement.
- #102 — Rust/Python/TypeScript SDK lockstep completion.
- #83 — `resource.open@1` HostAction scaffold/substrate.

Important commits on parent:

```text
50fb3f69 feat(extensions): enforce sdk contract compatibility
d392f23d feat(extensions): add resource open host action
```

#83 was closed as completed after the substrate work, with #125 split out for real operator-visible backend execution.

## Current workstream target

GitHub issue:

```text
#125 Implement real resource.open@1 backends for Flynt, Zed, and terminal reader
```

Design node:

```text
resource-open-real-backends-125
```

Design node file:

```text
docs/resource-open-real-backends-125.md
```

Design-node commit on this branch:

```text
e79dc531 docs(design): track resource open backend follow-up
```

## What already exists from #83 substrate

Implemented in parent commit `d392f23d`:

- `omegon-extension` updated to `0.25.2` in `Cargo.lock`.
- SDK-owned manifest permission model consumed in Omegon:
  - `manifest.permissions.host_actions.resource_open`
- `resource.open@1` HostAction type is recognized by the extension host pipeline.
- Policy validation exists for:
  - manifest allowlist
  - `resource_open.allow`
  - URI scheme
  - intent
  - kind
  - file root
- Secure `${workspace}` enforcement exists:
  - `${workspace}` resolves to executor workspace root.
  - `/etc/passwd` denied.
  - `../` parent escape denied.
- Backend seam exists:
  - `ResourceOpenBackend`
  - `ResourceBackendRegistry`
  - `UnavailableResourceOpenBackend`
  - `FakeResourceOpenBackend` for tests
- Routing skeleton exists:
  - markdown / diagram / image → Flynt
  - code / text / directory → Zed
  - ebook / pdf → Terminal
  - unknown / missing kind → Fallback
- Tests exist under `cargo test -p omegon resource_open`.

Validation previously passed:

```bash
cargo test -p omegon resource_open -- --nocapture
cargo test -p omegon host_actions -- --nocapture
cargo test -p omegon --test extension_install_blackbox -- --nocapture
cargo check -p omegon
git diff --check
```

## Known caveats from adversarial assessment

1. Real backends do not exist yet.
   - Current real/default registry uses unavailable/fallback behavior.
   - Fake backends prove routing only.

2. `file://` parsing is simple.
   - Current implementation uses string stripping, not a full URI parser.
   - Future real backend work should consider `url::Url` before accepting broad file URI semantics.

3. Existing path canonicalization is secure for existing paths and syntactic for non-existing paths.
   - Before supporting `edit`/create flows, account for symlinked parent directories.

4. Workspace root source should be reviewed.
   - HostAction executor currently carries `workspace_cwd`.
   - Some construction paths may still derive it from `std::env::current_dir()`.
   - For real backends, prefer explicit setup/workspace root threading if needed.

5. Routing trusts extension-provided `kind`.
   - Security does not depend on `kind`, but UX routing does.
   - Real backend work should infer or validate kind from URI/extension when possible.

## Recommended next target

Start #125 with implementation planning before coding.

First concrete design decisions to record in `docs/resource-open-real-backends-125.md`:

1. Backend ownership boundaries:
   - Flynt backend: host/ACP surface integration or unavailable outside Flynt.
   - Zed backend: CLI/app opener path, policy-gated.
   - Terminal reader backend: translate selected resources to `terminal.create@1` / Bookokrat when manifest/runtime policy permits.

2. Runtime availability model:
   - Backends should report unavailable explicitly rather than silently fallback.
   - Registry should preserve preferred backend selection but return warnings when falling back.

3. Resource URI parsing:
   - Decide whether to introduce `url::Url` now for file URI correctness.

4. Workspace root source:
   - Decide whether #125 must thread explicit workspace root through `ToolExecutionContext` / setup instead of relying on current executor construction.

5. Closure criteria for #125:
   - At least one real backend path per class or explicit reason for deferral.
   - Tests for successful real/fake backend selection and explicit fallback warnings.
   - Changelog update.
   - Broader validation.

## Suggested implementation sequence

1. Read current substrate:

```bash
rg -n "ResourceOpenBackend|ResourceBackendRegistry|preferred_resource_backend_kind|execute_resource_open" core/crates/omegon/src/extensions/host_actions.rs
```

2. Update design node with decisions/open questions.

3. Add backend availability abstractions:
   - backend `available()` / `diagnostics()` or unavailable outcome warnings.
   - preserve deterministic fallback.

4. Implement terminal/Bookokrat backend first.
   - It can likely reuse existing `terminal.create@1` machinery.
   - This gives operator-visible behavior without needing Flynt/Zed APIs first.

5. Implement or stub Flynt/Zed backends with explicit unavailable diagnostics if host integration is not present.

6. Add tests:
   - ebook/pdf routes to terminal backend.
   - markdown routes to Flynt when available, otherwise fallback with warning.
   - code routes to Zed when available, otherwise fallback with warning.
   - backend failure returns explicit typed outcome.

7. Validate:

```bash
cargo test -p omegon resource_open -- --nocapture
cargo test -p omegon host_actions -- --nocapture
cargo check -p omegon
git diff --check
```

8. Update `CHANGELOG.md`.

9. Commit.

## Branch/PR strategy

Preferred order:

1. Open/merge PR for `workstream/0.27-sdk-contract` first.
2. Continue #125 on `workstream/resource-open-real-backends-125`.
3. After parent branch merges, rebase #125 onto updated `main`.

If coding #125 before parent merge, target the PR at `workstream/0.27-sdk-contract` or clearly mark it as stacked.
