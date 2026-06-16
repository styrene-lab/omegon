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
[permissions]
bash = { action = "prompt", patterns = ["rm *", "sudo *"] }
edit = "allow"
write = { action = "prompt", patterns = ["*.env", "*.key", "*.pem"] }
read = "allow"
external_directory = { action = "deny", allow = ["/usr/local/include"] }
```

Actions: `allow` (silent), `prompt` (ask operator), `deny` (block).
Patterns: glob matching on tool arguments.

Runtime invariant: permission-required operations are not recoverable tool failures. They suspend the agent run until explicit operator allow, explicit deny, explicit run cancellation, or an upstream preapproval/bypass such as a trusted directory or `--dangerously-bypass-permissions`. A passive timeout must not convert a permission prompt into denial.

Bash mediation is static and advisory for common shell forms (redirects, `tee`, `cp`/`mv`/`install`, `mkdir`, `rm`). It unifies detected bash boundary hits with the permission prompt surface, but hard filesystem containment for shell variable indirection, subprocesses, and programmatic I/O belongs to the sandbox layer.

The key improvement over OpenCode: our permissions compose with personas. The tutor persona says "no bash" — that's a persona-level deny that the operator can't accidentally override with a session sticky. The Lex Imperialis could define absolute denies (e.g. never allow `rm -rf /`).

## Open Questions

- What is the permission config format — TOML file in .omegon/permissions.toml, inline in opencode.json-style config, or both?
