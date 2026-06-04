---
title: 0.27 Closeout Workstream Handoff
status: active
tags: [handoff, 0.27.0, release, workstreams]
date: 2026-06-03
---

# 0.27 Closeout Workstream Handoff

## Current repo state

Current branch at handoff:

```text
main
```

Working tree status at last check:

```text
clean
main == origin/main
```

Current `main` head:

```text
7df06d98 style(host-actions): format resource open tests
```

Workspace version is still:

```text
0.26.0
```

`CHANGELOG.md` still has the 0.27 payload under:

```text
## [Unreleased]
```

So: implementation work is landed, but the 0.27 release line is not wrapped yet.

## What just landed

The `workstream/0.27-sdk-contract` branch was fast-forward merged into `main` and pushed.

Landed commits over `v0.26.0` / previous `main`:

```text
50fb3f69 feat(extensions): enforce sdk contract compatibility
d392f23d feat(extensions): add resource open host action
e79dc531 docs(design): track resource open backend follow-up
9a45bd4f docs: add resource open backend handoff
13b3e083 docs(design): record resource open backend decisions
27ba797f fix(host-actions): report resource open backend diagnostics
625d35b5 feat(host-actions): route reader resources through terminal backend
3a2d4fd0 fix(host-actions): parse resource file uris with url
401eccff test(host-actions): cover real resource open registry wiring
9fd52a63 docs(design): update resource open backend status
b27467c8 docs(design): split flynt and zed resource open followups
7df06d98 style(host-actions): format resource open tests
```

Implemented scope:

- Extension SDK contract compatibility enforcement.
- `resource.open@1` HostAction substrate.
- Manifest policy validation for resource open.
- Secure `file://` workspace-root enforcement.
- `url::Url` file URI parsing, including encoded path handling and non-local host rejection.
- Resource backend registry seam.
- Preferred/selected backend unavailable diagnostics.
- Terminal/Bookokrat reader backend for ebook/pdf resources.
- Explicit unavailable diagnostics for Flynt/Zed/fallback backends.
- Follow-up design nodes for Flynt and Zed real backends.

Important follow-up docs now on `main`:

```text
docs/resource-open-real-backends-125.md
docs/resource-open-flynt-backend.md
docs/resource-open-zed-backend.md
docs/workstream-handoff-resource-open-real-backends-125.md
```

## Validation already run

Before/after landing this stack, the following passed:

```bash
cargo test -p omegon resource_open -- --nocapture
cargo test -p omegon terminal_create -- --nocapture
cargo check -p omegon
just lint
just test-rust
```

A first `just lint` run failed only on rustfmt for `host_actions.rs`; `cargo fmt --all` fixed it, and `just lint && just test-rust` then passed. The formatting-only fix was committed as:

```text
7df06d98 style(host-actions): format resource open tests
```

## Workstream branch status

Merged / landed branches:

```text
workstream/0.27-sdk-contract       -> same head as main
workstream/resource-open-real-backends-125 -> ancestor of main
```

Stale placeholder branches:

```text
workstream/0.27-host-actions
workstream/0.27-voice-agent-mode
```

Both stale placeholders point at the old `v0.26.0` base commit:

```text
c3a1fb4f Merge pull request #122 from styrene-lab/release/0.26
```

They have no unique commits ahead of `main`. Do not continue work from them as-is. Either delete them, or recreate/reset from current `main` if those names are needed later.

Suggested local cleanup after reboot, if desired:

```bash
git branch -d workstream/0.27-sdk-contract
git branch -d workstream/resource-open-real-backends-125
git branch -d workstream/0.27-host-actions
git branch -d workstream/0.27-voice-agent-mode
```

Do not delete remote branches until deciding whether to preserve the audit trail through the 0.27 release/tag.

## Current milestone assessment

The implementation work for the 0.27 foundation is done enough for this phase.

Accurate 0.27 scope:

```text
SDK compatibility + HostAction/resource.open substrate foundation
```

Not in scope for 0.27:

- Real Flynt resource backend.
- Real Zed/editor resource backend.
- Browser/default opener fallback.
- Native TUI/control-plane resource presentation.

Those are deferred because the TUI/control-plane work needs to settle the real integration surfaces first. The current explicit unavailable diagnostics are intentional, not a defect.

## Remaining closeout work

The release bookkeeping still needs to happen before starting the foundational assessment workstream.

Recommended next branch:

```bash
git checkout main
git pull --ff-only origin main
git checkout -b workstream/0.27-closeout
```

Then inspect release recipes:

```bash
just --list | rg 'release|rc|branch'
```

Expected closeout tasks:

1. Move the 0.27 payload out of `CHANGELOG.md` `[Unreleased]` into a 0.27 section.
2. Bump the workspace version from `0.26.0` to either:
   - `0.27.0-rc.1` if opening an RC/release line, or
   - `0.27.0` if cutting stable directly.
3. Prefer the repo's release recipe if available (`just rc`, `just release`, or `just branch-release`) rather than hand-rolling release mechanics.
4. Run release gates:

   ```bash
   just lint
   just test-rust
   just link
   ```

5. Commit closeout/release prep, likely:

   ```text
   chore(release): prepare 0.27.0-rc.1
   ```

6. Push the closeout branch or merge to `main` depending on the chosen release process.

## Why this closeout matters

Right now the repo has 0.27 functionality but still reports:

```text
Cargo.toml version = 0.26.0
CHANGELOG section = [Unreleased]
```

Starting the next foundational assessment workstream before fixing that would leave milestone state ambiguous. The next agent should first make the release/milestone state explicit, then move on.

## Next work after 0.27 closeout

Once the 0.27 release state is wrapped, start an assessment-first branch for foundational reliability work.

Suggested branch name:

```text
workstream/foundation-assessment
```

Suggested first deliverable:

```text
docs/foundation-assessment-agent-loop-provider-truncation.md
```

Assessment targets:

- Core agent loop failure and recovery surfaces.
- Provider/upstream error taxonomy and display.
- Truncation surfaces:
  - provider context truncation,
  - internal context compaction/trimming,
  - tool output truncation,
  - TUI visual truncation,
  - final answer/token truncation.
- Rendering/component boundaries that extensions can safely use.

Do assessment first; do not jump directly into implementation fixes until the surfaces and ownership boundaries are mapped.
