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

## 5. Path dialect classification
<!-- specs: permissions/dialects-and-environments -->

- [x] 5.1 Add `PathDialect` and dialect-aware `PathTarget` variants for POSIX, Windows, WSL, MSYS, and Cygwin path shapes.
- [x] 5.2 Ensure Windows drive-absolute, drive-relative, UNC, verbatim, and device namespace paths are never classified as workspace-relative by fallback.
- [x] 5.3 Add WSL `/mnt/<drive>`, MSYS `/<drive>`, and Cygwin `/cygdrive/<drive>` diagnostics without auto-translating paths.
- [x] 5.4 Add classifier tests for Windows, WSL, MSYS, Cygwin, and POSIX edge cases.

## 6. Sensitive infrastructure path classification
<!-- specs: permissions/dialects-and-environments -->

- [ ] 6.1 Add path warnings/risks for Kubernetes service account tokens, projected secrets, and `/run/secrets` material.
- [ ] 6.2 Add path warnings/risks for Docker, Podman, and containerd runtime sockets.
- [ ] 6.3 Add path warnings/risks for dangerous `/proc`, `/sys`, and `/dev` runtime/kernel material.
- [ ] 6.4 Add path warnings/risks for XDG document portal and sandbox-private storage paths.

## 7. Environment and mount context
<!-- specs: permissions/dialects-and-environments -->

- [ ] 7.1 Add best-effort `EnvironmentContext` detection for Docker-like containers, Kubernetes pods, devcontainers, WSL, Flatpak, Snap, and VM guests.
- [ ] 7.2 Parse Linux `/proc/self/mountinfo` into `MountContext` when available.
- [ ] 7.3 Classify overlayfs, bind mounts, Docker volumes, Kubernetes projected volumes, VM shared folders, FUSE, and XDG document portal mounts.
- [ ] 7.4 Attach mount/environment context to resolved filesystem targets and permission diagnostics.

## 8. Trust grant context
<!-- specs: permissions/dialects-and-environments -->

- [ ] 8.1 Distinguish `TrustedExternal` from `InsideWorkspace` in resolved relations and mediation copy.
- [ ] 8.2 Record mount/environment identity with persistent trusted-directory grants where available.
- [ ] 8.3 Re-prompt or warn when a trusted path resolves to a different mount identity than the one originally approved.
