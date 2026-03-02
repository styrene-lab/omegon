---
name: cleave
description: Recursive task decomposition. Routes directives to cleave CLI based on
  complexity assessment. Use for multi-system implementations.
---

# Cleave

Deterministic routing of complex directives to the `cleave` CLI orchestration engine. This skill does not construct prompts or dispatch subagents directly — it invokes CLI commands and presents results at each gate for user approval.

## Prerequisite

Verify the CLI is installed:

```bash
which cleave
```

If missing, tell user: `pipx install styrene-cleave`

## State Machine

```
PREFLIGHT -> ASSESS -> ROUTE -> PLAN -> REVIEW -> EXECUTE -> REPORT
                                                   ^          |
                                                   +-- GATE --+
```

Follow each state sequentially. Do not skip states. Wait for user input at every gate.

### 1. PREFLIGHT

Check the target repo for uncommitted changes:

```bash
git -C <repo> status --porcelain
```

- If dirty and user did not specify `--dirty`: ask user to commit or acknowledge `--dirty` flag
- If clean: proceed

### 2. ASSESS

Run the assessment CLI:

```bash
cleave assess -d "<directive>" -f json
```

Extract from JSON output:
- `decision`: "execute" or "cleave"
- `complexity`: numeric score
- `systems`: system count
- `pattern`: matched pattern name (if any)
- `confidence`: pattern confidence

Present the assessment summary to the user.

### 3. ROUTE

Determine execution tier from the assessment:

| decision   | complexity | Tier |
|------------|-----------|------|
| `"execute"` | any       | **Direct** — execute in-session, no cleave |
| `"cleave"` | < 12      | **Orchestrator** — `cleave run` |
| `"cleave"` | >= 12     | **Architect** — `cleave architect` |

**User override**: If the directive contains "architect" or "multi-phase", use Architect tier regardless.

Present the routing decision with assessment data. **Confirm with user before proceeding.**

If tier is **Direct**: execute the task normally in-session. The remaining states do not apply.

### 4. PLAN

Generate a plan and pause for review.

**Orchestrator tier:**

```bash
cleave run -d "<directive>" -r <repo> \
  -s "<success criterion 1>" \
  -s "<success criterion 2>" \
  --confirm -f json \
  --model opus --max-budget 50
```

**Architect tier:**

```bash
cleave architect -d "<directive>" -r <repo> \
  -s "<success criterion 1>" \
  -s "<success criterion 2>" \
  --plan-only -f json \
  --planner-model opus --max-budget 200
```

Parse the JSON output. Extract:
- `plan_review_path`: path to the plan review file
- `resume_command`: command to resume after review
- `workspace_path` or `db_path`: workspace location

### 5. REVIEW

Read the plan review file:

```
Read tool: <plan_review_path>
```

Present the full plan to the user. Wait for one of:
- **Approve**: proceed to EXECUTE
- **Modify**: discuss changes; user edits plan files manually, then approve
- **Cancel**: stop — present workspace path for manual cleanup

### 6. EXECUTE

Run the resume command with JSON output:

**Orchestrator:**
```bash
cleave run --resume <workspace_path> -f json
```

**Architect:**
```bash
cleave architect --resume <db_path> -f json
```

Run via Bash tool. These commands can run for extended periods — use a generous timeout.

### 7. GATE

Parse JSON output from EXECUTE:

| Condition | Action |
|-----------|--------|
| `success && !paused` | Proceed to REPORT |
| `paused` | Read `report_path`, present checkpoint to user. Ask: continue or abort. Continue -> back to EXECUTE with resume_command. Abort -> REPORT with partial results. |
| `!success` | Read `report_path`, present error to user. Ask: retry or abort. Retry -> back to EXECUTE. Abort -> REPORT. |

### 8. REPORT

Read the report file:

```
Read tool: <report_path>
```

Present to user:
- Status (success/partial/failed)
- Cost (total_cost_usd)
- Phases or children completed vs total
- Any errors or failures
- Workspace/database path for manual inspection

## Error Handling

| Condition | Action |
|-----------|--------|
| CLI not found | Tell user to install: `pipx install styrene-cleave` |
| Non-zero exit with no JSON on stdout | Read stderr, present error to user |
| Budget exhaustion | Present cost summary, ask user whether to increase budget and retry |
| Circuit breaker tripped | Present failure count and errors, ask user |

## Subprocess Isolation (Autonomous Modes)

Children spawned by `cleave run` and `cleave architect` are capability-stripped:

| Capability | Status | Detail |
|-----------|--------|--------|
| Built-in tools | `Bash Edit Read Write Glob Grep` | Explicit allowlist |
| MCP servers | **None** | `--strict-mcp-config` prevents inheritance |
| Skills/commands | **None** | `--disable-slash-commands` |
| Plugins/hooks | **None** | `--print` mode |
| WebFetch/WebSearch | **Unavailable** | Not in allowed tools |
| Task tool | **Unavailable** | No subagent spawning |
| Network via Bash | Unrestricted | `curl`, `wget` available |

**Implication**: Context requiring web research or MCP queries must be gathered before invoking PLAN and embedded in the directive or success criteria.

## Command Reference

| Command | Purpose |
|---------|---------|
| `cleave assess -d "<directive>" -f json` | Complexity assessment |
| `cleave run -d "<directive>" -r <repo> --confirm -f json` | Plan + pause |
| `cleave run --resume <workspace> -f json` | Resume orchestrator |
| `cleave architect -d "<directive>" -r <repo> --plan-only -f json` | Architect plan |
| `cleave architect --resume <db_path> -f json` | Resume architect |
| `cleave probe -d "<directive>" -r <repo>` | Codebase interrogation |
| `cleave check-permissions -d "<directive>" --snippet` | Permission gaps |

### `cleave run` flags

| Flag | Default | Description |
|------|---------|-------------|
| `-d, --directive` | (required) | Top-level task directive |
| `-r, --repo` | cwd | Path to target repository |
| `-s, --success-criteria` | [] | Success criterion (repeatable) |
| `-f, --format` | text | Output format: text or json |
| `--model` | opus | Model for child executors |
| `--planner-model` | sonnet | Model for planning phase |
| `--max-budget` | 50 | Total budget in USD |
| `--child-budget` | 15 | Per-child budget in USD |
| `--timeout` | 8h | Total timeout |
| `--child-timeout` | 2h | Per-child timeout |
| `--max-depth` | 3 | Max recursion depth (1-10) |
| `--circuit-breaker` | 3 | Consecutive failures before halt |
| `--max-parallel` | 4 | Max parallel children |
| `--mcp-config` | "" | MCP config JSON or path for children (empty = no MCP) |
| `--dry-run` | false | Plan only, don't dispatch |
| `--confirm` | false | Stop after planning for review |
| `--resume` | -- | Resume from workspace path |
| `--dirty` | false | Allow dirty working tree |
| `--verbose` | false | Debug logging |

### `cleave architect` flags

| Flag | Default | Description |
|------|---------|-------------|
| `-d, --directive` | (required) | Project-level directive |
| `-r, --repo` | cwd | Path to target repository |
| `-s, --success-criteria` | [] | Success criterion (repeatable) |
| `-f, --format` | text | Output format: text or json |
| `--plan-only` | false | Decompose only, don't execute |
| `--resume` | -- | Resume from architect.db path |
| `--max-budget` | 200 | Total budget in USD |
| `--phase-budget` | 50 | Per-phase budget in USD |
| `--child-budget` | 15 | Per-child budget within orchestrator |
| `--timeout` | 24h | Total timeout |
| `--phase-timeout` | 8h | Per-phase timeout |
| `--planner-model` | opus | Model for architect planning |
| `--model` | opus | Model for child execution |
| `--executor-planner-model` | sonnet | Model for orchestrator split planning |
| `--max-phases` | 10 | Maximum number of phases |
| `--max-depth` | 3 | Max cleave recursion within each phase |
| `--circuit-breaker` | 2 | Consecutive phase failures before halt |
| `--max-parallel` | 4 | Max parallel children within orchestrator |
| `--confirm` | false | Pause after planning for review |
| `--dirty` | false | Allow dirty working tree |
| `--verbose` | false | Debug logging |
