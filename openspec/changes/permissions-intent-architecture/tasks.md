# Tasks

## 1. Intent and resolution model
<!-- specs: permissions/intent -->

- [x] 1.1 Add internal `FsIntent`, `FsOperation`, `PathTarget`, `IntentSource`, and `IntentConfidence` types under the core tools/permissions module boundary.
- [x] 1.2 Add resolved-target and warning classifiers for root-dot paths (`/.omegon/...`) and suspicious short root paths (`/Ig`).
- [x] 1.3 Keep existing `WorkspaceBoundary` behavior strict; do not auto-correct absolute paths.

## 2. Bash and terminal extraction
<!-- specs: permissions/shell-intent -->

- [x] 2.1 Replace direct `scan_boundary_violations` internals with shell filesystem intent extraction while preserving a compatibility wrapper.
- [x] 2.2 Include operation/source/confidence metadata for redirects, `tee`, `cp/mv/install`, `mkdir`, and `rm`.
- [x] 2.3 Update bash and terminal preflight to evaluate structured intents before execution.

## 3. Suspicious-path mediation
<!-- specs: permissions/mediation -->

- [x] 3.1 Convert low-confidence suspicious scanner hits into blocked diagnostics rather than ordinary persistent permission prompts.
- [x] 3.2 Add correction-oriented diagnostics for `/.omegon`-class paths that suggest workspace-relative `.omegon` without rewriting the command.
- [x] 3.3 Ensure legitimate exact outside-workspace paths continue to use the existing approval flow.

## 4. Tests and release hygiene
<!-- specs: permissions/intent, permissions/shell-intent, permissions/mediation -->

- [x] 4.1 Add focused unit tests for `.omegon` vs `/.omegon` resolution diagnostics.
- [x] 4.2 Add focused unit tests for `/Ig`-class suspicious scanner hits.
- [x] 4.3 Add regression tests for legitimate `/etc/...`, `/tmp/...`, trusted-directory, and standard file descriptor/device paths.
- [x] 4.4 Update `CHANGELOG.md` `[Unreleased]` to identify permissions intent architecture as the primary 0.27.8 target.
