+++
id = "6f83d628-0d4e-4b7d-9761-a7f12bc5eec6"
kind = "document"
title = "Granular tool permissions — per-tool, per-path allow/deny/prompt policies"
status = "exploring"
tags = ["security", "permissions", "tools", "ux", "policy"]
aliases = ["granular-permissions"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
issue_type = "feature"
open_questions = ["What is the permission config format — TOML file in .omegon/permissions.toml, inline in opencode.json-style config, or both?"]
priority = "1"
+++

# Granular tool permissions — per-tool, per-path allow/deny/prompt policies

## Overview

Replace the current coarse tool enable/disable with a granular permission system: per-tool actions (allow/deny/prompt), per-path patterns with wildcards, external_directory guards, and persona-scoped permission overrides. OpenCode's permission model is the benchmark — we need parity plus integration with our persona and Lex Imperialis layers.

## Research

### Permission model design — layered with personas and Lex Imperialis

OpenCode's model: per-tool permission with allow/deny/prompt actions, wildcard path patterns, external_directory guard, session-level "always allow" sticky approvals.

Omegon's model should layer with the persona system:

```
Lex Imperialis (non-overridable)
  → Persona tool profile (disable bash for tutor)
    → Project permissions (per-path patterns)
      → Operator session overrides (sticky approvals)
```

Proposed permission schema for plugin.toml and project config:
```toml
[permissions.tools]
bash = { action = "prompt", patterns = ["rm *", "sudo *"] }
edit = "allow"
write = { action = "prompt", patterns = ["*.env", "*.key", "*.pem"] }
read = "allow"

[permissions.external_directory]
action = "deny"
allow = ["/usr/local/include"]
```

Actions: `allow` (silent), `prompt` (ask operator), `deny` (block).
Patterns: glob matching on tool arguments.

Runtime invariant: permission-required operations are not recoverable tool failures. They suspend the agent run until explicit operator allow, explicit deny, explicit run cancellation, or an upstream preapproval/bypass such as a trusted directory or `--dangerously-bypass-permissions`. A passive timeout must not convert a permission prompt into denial.


Policy prompts are allow-once in this slice: host/TUI `allow always` selections are normalized to the same one-shot `allow` decision because there is not yet a durable policy grant target. Persistent directory trust remains limited to workspace-boundary prompts.

Subject extraction contract for the initial engine slice:

| Tool | Matched subject | Sensitive values matched? |
|---|---|---|
| `bash`, `terminal` | `command` | command text only |
| `read`, `write`, `edit` | `path` | no file contents |
| `change` | each `edits[].file` | no old/new content |
| `validate` | each `paths[]` | no validation output |
| `secret_set`, `secret_delete` | secret `name` | never secret values |
| `web_fetch` | `url` | URL only |

Multi-subject tools aggregate to the strongest decision: `deny > prompt > allow`. Layering is a monotonic tightening lattice, not last-writer-wins: Lex, persona, project, and session policies are all evaluated, and lower layers may tighten an invocation but cannot loosen a higher-layer prompt or deny. A session `allow` therefore cannot bypass a project/persona/Lex `prompt` or `deny`.

Unknown tools and tools without extracted subjects are currently default-open unless a tool-level rule exists. This preserves extension compatibility for the first enforcement slice, but it is not a security boundary; restrictive deployments should add explicit tool rules until a configurable unknown-tool default exists.

Pattern matching is lexical and glob-like (`*` any sequence, `?` one character). Path patterns are matched against extracted argument strings, not canonicalized filesystem paths; normalization/canonical path policy belongs to a later path-aware matcher.

Bash mediation is static and advisory for common shell forms (redirects, `tee`, `cp`/`mv`/`install`, `mkdir`, `rm`). It unifies detected bash boundary hits with the permission prompt surface, but hard filesystem containment for shell variable indirection, subprocesses, and programmatic I/O belongs to the sandbox layer.

The key improvement over OpenCode: our permissions compose with personas. The tutor persona says "no bash" — that's a persona-level deny that the operator can't accidentally override with a session sticky. The Lex Imperialis could define absolute denies (e.g. never allow `rm -rf /`).

## Operator prompt controls (shipped in 0.28.7)

Workspace-boundary prompts suspend the active run and show the tool, exact target, and proposed directory. The operator selects an explicit grant scope:

| Key | Scope | Runtime effect |
|---|---|---|
| `y` | one operation | Approves only the requested canonical target; sibling files are not implicitly trusted. |
| `a` | current session | Trusts the proposed directory until the Omegon process exits. |
| `Shift+A` | project | Persists the proposed directory in the active project profile for future sessions. |
| `n` / `Esc` | deny | Rejects the operation without adding trust. |

These directory scopes apply to workspace-boundary prompts. Policy prompts without a durable grant target remain allow-once.

Operators can deliberately bypass Omegon's interactive tool-policy and filesystem-boundary mediation for one process:

```bash
omegon --dangerously-bypass-permissions
```

This does not persist trusted directories or override operating-system permissions, macOS privacy controls, container/mount boundaries, missing credentials, or upstream API authorization. Child runs inherit the bypass posture. Use it only where the operator accepts the consequences of every tool call.

## Open Questions

- What is the permission config format — TOML file in .omegon/permissions.toml, inline in opencode.json-style config, or both?
