+++
id = "0548fb03-5795-4a7c-b912-168679d1999c"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# bridge-all-slash-commands — Design

## Spec-Derived Architecture

### harness/slash-command-bridge

- **All OpenSpec commands are agent-callable via execute_slash_command** (added) — 5 scenarios
- **Interactive-only commands are registered but not agent-callable** (added) — 1 scenarios
- **Bridge registration preserves existing interactive UX** (added) — 1 scenarios
- **Bridge metadata declares correct side-effect classes** (added) — 2 scenarios

## Scope

**In scope:**
- All 7 OpenSpec commands: /opsx:propose, /opsx:spec, /opsx:ff, /opsx:status, /opsx:verify, /opsx:archive, /opsx:apply
- /dashboard registered with agentCallable: false
- Any other clearly interactive-only commands (e.g. /dash alias)

**Out of scope:**
- /cleave — already has cleave_run tool as agent-callable path; bridging its full interactive workflow is a separate effort
- /assess — already bridged
- Utility commands from other extensions (auth, chronos, whoami, etc.) — these have dedicated tools already

## Design Decisions

### Share the SlashCommandBridge instance across extensions

The cleave extension currently creates its own bridge with `createSlashCommandBridge()`. OpenSpec commands need a bridge too. Rather than creating a second bridge (which would split the tool's command list), export a shared singleton from a common location. The cleave extension's `createToolDefinition()` call already registers the `execute_slash_command` tool — OpenSpec commands just need to `.register()` on the same bridge instance before that tool is created.

**Approach:** Export the bridge instance from `extensions/lib/slash-command-bridge.ts` as a module-level singleton, or pass it between extensions via shared state. The simplest path: create the bridge in shared state and have both cleave/index.ts and openspec/index.ts register their commands on it.

### structuredExecutor extracts logic from existing handlers

Each OpenSpec command handler already contains the business logic. The conversion pattern is:
1. Extract the core logic into a `structuredExecutor` that returns `SlashCommandBridgeResult`
2. The `interactiveHandler` calls the executor and renders notifications/messages from the result
3. The bridge registers both, routing interactive input through the same executor

For commands that use `ctx.ui.input()` (like /opsx:propose asking for intent), the structuredExecutor should accept the intent as an arg or return a structured prompt. The bridge can handle this via the `interactiveHandler` path.

## File Changes

- `extensions/openspec/index.ts` (modified) — Convert all 7 OpenSpec commands + /opsx:apply from plain registerCommand to bridge.register with structuredExecutors
- `extensions/openspec/bridge.test.ts` (new) — Test bridged OpenSpec commands return structured results, correct metadata, and agent-callable status
- `extensions/cleave/index.ts` (modified) — Import shared bridge instance instead of creating a local one; register /assess on the shared bridge
- `extensions/dashboard/index.ts` (modified) — Register /dashboard and /dash with bridge (agentCallable: false)
- `extensions/lib/slash-command-bridge.ts` (modified) — Export a shared bridge singleton or factory that multiple extensions can register on
- `extensions/shared-state.ts` (modified) — Optionally store bridge reference if cross-extension sharing needs shared state
