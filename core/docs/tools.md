# Tools

Tools are capabilities the agent can invoke during a session. They are dispatched through the EventBus and executed by the feature that registered them.

## Core tools

| Tool | Description |
|------|-------------|
| `bash` | Execute shell commands. Supports timeout. |
| `read` | Read file contents with optional offset/limit. Supports images. |
| `write` | Create or overwrite files. Creates parent directories. |
| `edit` | Surgical text replacement — find exact text and replace. |
| `change` | Multi-file atomic edits with validation. |
| `view` | Render files inline — images, PDFs, documents, code with syntax highlighting. |
| `whoami` | Check auth status across dev tools (git, GitHub, AWS, k8s, OCI). |
| `chronos` | Authoritative date/time — week, month, quarter, relative, epoch, timezone. |
| `web_search` | Search the web via Brave, Tavily, or Serper. Quick, deep, or compare modes. |

## Speculate tools

Git-checkpoint-based exploration — try changes without committing to them.

| Tool | Description |
|------|-------------|
| `speculate_start` | Begin a speculation — snapshots current state. |
| `speculate_check` | Review speculation status and diff. |
| `speculate_commit` | Accept speculation — merge into working tree. |
| `speculate_rollback` | Discard speculation — restore snapshot. |

## Local inference

| Tool | Description |
|------|-------------|
| `ask_local_model` | Delegate to local LLM via Ollama. Zero API cost. |
| `list_local_models` | List available Ollama models. |
| `manage_ollama` | Start, stop, check status, pull models. |

## Model & thinking controls

| Tool | Description |
|------|-------------|
| `set_model_tier` | Switch capability tier — local, retribution, victory, gloriana. |
| `set_thinking_level` | Adjust reasoning budget — off, low, medium, high. |
| `switch_to_offline_driver` | Switch from cloud to local Ollama model. |

Shortcut aliases: `opus`, `sonnet`, `haiku`, `gloriana`, `victory`, `retribution`.

## Lifecycle tools

| Tool | Description |
|------|-------------|
| `design_tree` | Query the design tree — list nodes, get details, find open questions. |
| `design_tree_update` | Mutate the design tree — create nodes, set status, add decisions/research. |
| `openspec_manage` | Manage OpenSpec changes — propose, spec, fast-forward, verify, archive. |
| `focus` / `unfocus` | Set which design node's context is injected into the conversation. |

## Decomposition

| Tool | Description |
|------|-------------|
| `cleave_assess` | Assess task complexity — returns score and execute/cleave decision. |
| `cleave_run` | Execute a parallel decomposition plan in git worktrees. |

## Management

| Tool | Description |
|------|-------------|
| `manage_tools` | List, enable, or disable tools for context window management. |
