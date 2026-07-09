# Permissions Intent Architecture

## Intent

Make the permissions system the main 0.27.8 release target by repositioning it around structured filesystem intent, provenance, path-resolution diagnostics, and confidence-aware mediation instead of raw path-string prompts.

## Problem

Field evidence shows the current permission boundary can ask operators to approve nonsensical or misleading paths such as `/Ig` and `/.omegon`. These are not legitimate operator decisions:

- `/Ig` is likely a low-confidence bash scanner extraction artifact from raw command text.
- `/.omegon` is a host-root absolute path that often indicates a mistaken current-working-directory/path construction bug where `.omegon` under the workspace was intended.

The current `PathPermissionError` carries only `requested_path`, `directory`, and `workspace`, so the prompt loses provenance: operation, actor, source command excerpt, extraction confidence, and suspicious-path diagnostics.

## Scope

In scope for 0.27.8:

- Introduce internal filesystem intent and provenance types.
- Reposition bash/terminal boundary scanning as intent extraction rather than direct path violation reporting.
- Add path-resolution diagnostics for suspicious host-absolute paths, especially root-dot paths like `/.omegon` and short root paths like `/Ig`.
- Make low-confidence suspicious scanner hits block with diagnostic context instead of becoming ordinary permission approval prompts.
- Preserve strict boundary behavior: do not auto-correct absolute paths into workspace-relative paths.
- Improve permission prompt/tool result evidence so operators can see why a path was requested.

Out of scope for the first 0.27.8 slice:

- Full shell sandboxing.
- Automatic path rewriting.
- Replacing all shell heuristics with a complete shell parser in one step.
- Broad UX redesign unrelated to filesystem permissions.

## Success Criteria

- Permission checks are driven by structured `FsIntent` data for bash/terminal preflight and at least one exact tool path.
- Suspicious `/.omegon/...` paths produce a diagnostic that names the host-absolute path and suggests workspace-relative `.omegon/...` without rewriting it.
- Suspicious low-confidence `/Ig`-class shell scanner hits do not create normal persistent permission prompts.
- Bash and terminal permission diagnostics include source/provenance sufficient to identify the scanner rule or command excerpt.
- Existing legitimate outside-workspace permission flows continue to work.
