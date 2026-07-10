+++
title = "Operator shell observations"
tags = ["openspec","shell","conversation"]
+++

# Operator shell observations

# Operator shell observations

## Intent

Make operator-run `!` commands durable, model-visible evidence with honest operator provenance, and render their stdout/stderr as terminal output rather than Bash source.

## Scope

- Add a canonical operator tool-observation representation to conversation state.
- Commit completed `!` command records with command, cwd, result, exit/error status, duration, and provenance.
- Project those observations into provider-safe user-role model context.
- Persist and restore observations through the existing session format.
- Expose provenance to semantic conversation surfaces.
- Share ANSI-aware terminal output rendering between live and completed Bash output.
- Add regression tests and release memory.

## Out of scope

- PTY allocation or interactive subprocess input.
- Forcing color from programs that disable it when stdout is captured.
- Treating operator execution as assistant authorship.
- General redesign of all tool result renderers.

## Success criteria

- A subsequent model turn can recover the operator-run command, cwd, exit status, and bounded output.
- Provider requests contain no fabricated or orphaned tool-call/result pair.
- Session save/reload preserves the observation.
- Live and completed ANSI output retain equivalent styling and sanitize unsupported controls.
- Plain Bash output is neutral terminal text rather than Bash-source highlighted.
- TUI/semantic projections visibly retain operator provenance.
