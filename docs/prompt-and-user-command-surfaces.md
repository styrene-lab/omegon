# Prompt and User Command Surfaces

Reusable prompts are content. Slash commands are invocation surfaces. Keep those concepts separate so prompt libraries can grow without polluting the command namespace.

## Prompt definitions

Prompt definitions are Markdown files with optional TOML frontmatter.

Locations:

```text
prompts/*.md                 # bundled prompts
~/.omegon/prompts/*.md       # user prompts
<project>/.omegon/prompts/*.md # project-local prompts
```

Example:

```markdown
+++
title = "Daily Review"
description = "Summarize today's work"
tags = ["planning"]
aliases = ["review"]
+++

Summarize today's completed work, open risks, and next actions.
```

The canonical command-palette-native prompt router is:

```text
/prompt list
/prompt <name>          # shorthand for preview
/prompt get <name>
/prompt preview <name>
/prompt run <name>
/prompt submit <name>
/prompt delete <name>
```

`/prompt <name>` is intentionally shorthand for preview. It does not make every prompt a global slash command.

## Safety boundary

Prompt preview and resolution report a safety verdict:

```text
Clean
Suspicious { reasons }
Blocked { reasons }
```

The current guardrails are deliberately conservative:

- secret-like markers are blocked;
- instruction-override phrases are suspicious;
- blocked prompts are not returned through `/prompt run` or `/prompt submit`.

Direct execution/queueing over ACP/RPC is not silently enabled. `_prompts/preview` and `_prompts/resolve` are read surfaces. `_prompts/submit` currently resolves at the preview/queue boundary and requires a stronger confirmation/trust flow before it should enqueue turns directly.

## User-defined command aliases

Use user commands when you want direct slash invocation such as:

```text
/review
/standup
/handoff
```

Command aliases are explicit TOML definitions that target prompts. Locations:

```text
~/.omegon/commands/*.toml
<project>/.omegon/commands/*.toml
```

Example:

```toml
name = "review"
description = "Preview the daily review prompt"
target = "prompt:daily-review"
mode = "preview"

[availability]
tui = true
cli = true
acp = false

[safety]
class = "queue_mutation"
requires_confirmation = true
prompt_injection_sensitive = true
```

This registers `/review` as a command palette entry that previews `prompt:daily-review` with provenance and a safety verdict.

First-slice limits:

- only `target = "prompt:<id>"` is supported;
- only `mode = "preview"` is supported;
- built-in command names cannot be overridden;
- ACP is disabled by default unless explicitly enabled in the command definition.

## Design rule

> Prompt IDs are reusable content references. User commands are executable invocation surfaces.

Use `/prompt <name>` for quick canonical prompt preview. Use `.omegon/commands/*.toml` when a prompt deserves a first-class direct command like `/review`.
