+++
id = "0d35a730-3bdc-4487-9cc2-bce802b0926d"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Subagent architecture — Tasks

## 1. Delegate engine + feature (features/delegate.rs)

- [x] 1.1 AgentSpec: parse .omegon/agents/*.md YAML frontmatter + body as system prompt
- [x] 1.2 scan_agents(): startup scan, returns Vec<AgentSpec>
- [x] 1.3 DelegateResultStore: thread-safe HashMap<task_id, DelegateTask> with store/get/list/update
- [x] 1.4 DelegateRunner: spawn child, write detection (worktree for write agents), in-place for read-only
- [x] 1.5 DelegateFeature: delegate tool (sync+async), delegate_result, delegate_status
- [x] 1.6 provide_context: inject available agent names + descriptions
- [x] 1.7 on_event: toast on async delegate completion

## 2. Wiring

- [x] 2.1 features/mod.rs: register delegate module
- [x] 2.2 setup.rs: register DelegateFeature with cwd + agents
- [x] 2.3 tui/mod.rs: /delegate command in COMMANDS table + handler

## 3. Tests

- [x] 3.1 parse_agent_spec, scan_agents, delegate_feature_tools
- [x] 3.2 delegate_result_nonexistent, sync_delegate_unknown_agent
- [x] 3.3 provide_context_lists_agents

## 4. Constraints

- [x] 4.1 Read-only agents run in-place (no worktree)
- [x] 4.2 Write agents always get worktree
- [x] 4.3 Max 4 concurrent async delegates
- [x] 4.4 Sync returns ToolResult, async returns task_id
