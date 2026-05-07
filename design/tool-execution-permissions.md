+++
id = "ed3494a2-6c88-4983-b92c-30f809780c3d"
kind = "design_node"
title = "Tool execution permissions — configurable approval for sensitive operations"
status = "exploring"
tags = ["permissions", "security", "tools", "ux", "configuration"]
aliases = ["tool-execution-permissions"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
issue_type = "feature"
open_questions = ["Should bash commands get per-command approval or per-session blanket approval?", "Should permissions be per-project (.omegon/profile.json) or global (~/.omegon/profile.json)?", "How do child workers (cleave/delegate) inherit permission state?"]
parent = "junior-onramp-progressive-disclosure"
priority = "2"
+++

# Tool execution permissions — configurable approval for sensitive operations

## Problem

Omegon's tools currently have a binary permission model:
- **Inside workspace**: everything allowed, no confirmation
- **Outside workspace**: blocked entirely (now with interactive TUI prompt)

This misses several important cases:

1. **Destructive bash commands** — `rm -rf`, `git reset --hard`, `docker rm` execute without confirmation. Claude Code prompts for these. Omegon doesn't.
2. **Network access** — `web_search`, `web_fetch`, `curl` via bash have no opt-in/opt-out.
3. **Git operations** — `commit`, `push` via bash happen without user review.
4. **System modifications** — package installs, service management, cron changes.
5. **Cost-bearing operations** — `ask_local_model` (GPU time), inference on expensive tiers.

The `trusted_directories` system solved path-based permissions. This design extends the pattern to **operation-based permissions**.

## Design

### Permission levels

Three levels, matching Claude Code's model:

| Level | Behavior | Example |
|-------|----------|---------|
| **Allow** | Execute without prompting | `read`, `edit`, `write` (in workspace) |
| **Ask** | TUI prompt before execution | `bash rm`, `git push`, `write` (outside workspace) |
| **Deny** | Block entirely, return error | Tools disabled by posture |

### Configuration via profile.json

```json
{
  "permissions": {
    "bash": "ask_destructive",
    "write_outside_workspace": "ask",
    "git_push": "ask",
    "web_search": "allow",
    "commit": "allow",
    "install_packages": "deny"
  }
}
```

Built-in presets:

| Preset | Bash | Git Push | Web | Outside WS | Installs |
|--------|------|----------|-----|------------|----------|
| **open** (default) | allow | allow | allow | ask | allow |
| **cautious** | ask_destructive | ask | allow | ask | ask |
| **strict** | ask | ask | ask | ask | deny |

Set via `/permissions preset cautious` or per-tool overrides.

### Destructive bash detection

Instead of prompting for every bash command, detect destructive patterns:

```
rm -rf, rm -r, git reset --hard, git push --force, git checkout -- .,
docker rm, docker rmi, kubectl delete, DROP TABLE, truncate,
systemctl stop, kill -9, pkill, chmod -R, chown -R
```

`bash: "ask_destructive"` means: allow most commands, prompt only for detected destructive patterns. This is the sweet spot — not too noisy, catches the dangerous ones.

### TUI prompt (reuses existing PermissionRequest)

```
🔒 bash wants to run: rm -rf ./build/
   [y] allow   [a] always allow   [n] deny
```

Same interactive prompt as the workspace permission system. Same key handling, same `PermissionResponse` type.

### Slash command

```
/permissions                    — show current settings
/permissions preset cautious    — apply a preset
/permissions bash ask           — override one tool
/permissions git_push allow     — override one tool
```

Persisted to profile.json via the existing `capture_from` / `apply_to` pipeline.

### Pkl schema

```pkl
class PermissionConfig {
  bash: ("allow"|"ask"|"ask_destructive"|"deny")?
  write_outside_workspace: ("allow"|"ask"|"deny")?
  git_push: ("allow"|"ask"|"deny")?
  web_search: ("allow"|"ask"|"deny")?
  commit: ("allow"|"ask"|"deny")?
  install_packages: ("allow"|"ask"|"deny")?
}

permissions: PermissionConfig?
```

## Implementation order

1. Add `PermissionConfig` struct to settings.rs
2. Add destructive bash detection to tools/bash.rs
3. Wire into the existing `PermissionRequest` / `PermissionResponse` TUI flow
4. Add `/permissions` slash command
5. Add to Profile persistence + Pkl schema

## Relationship to existing systems

- **trusted_directories** — path-based. This design is operation-based. They're orthogonal.
- **posture tool filtering** — hides tools from the LLM entirely. Permissions allow the tool but require confirmation. Different layers.
- **SecretsManager GuardDecision** — blocks tools that would expose secrets. Permissions block based on user preference, not security policy.

## Non-goals

- Per-file permissions (use trusted_directories)
- Network firewall rules (out of scope — use OS-level controls)
- Auditing/logging of permitted operations (future work)
