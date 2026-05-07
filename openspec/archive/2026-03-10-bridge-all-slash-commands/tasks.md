+++
id = "245736c4-f493-4209-9ab0-2e54f3e362a0"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# bridge-all-slash-commands — Tasks

## 1. Bridge OpenSpec commands
<!-- specs: harness/slash-command-bridge -->
<!-- skills: typescript -->

- [x] 1.1 Create a shared SlashCommandBridge instance in extensions/lib/slash-command-bridge.ts via getSharedBridge()
- [x] 1.2 Convert /opsx:status to bridged command with structuredExecutor returning changes array
- [x] 1.3 Convert /opsx:verify to bridged command with structuredExecutor returning verification substate
- [x] 1.4 Convert /opsx:archive to bridged command with structuredExecutor (sideEffectClass: workspace-write)
- [x] 1.5 Convert /opsx:propose to bridged command with structuredExecutor (sideEffectClass: workspace-write)
- [x] 1.6 Convert /opsx:spec to bridged command with structuredExecutor (sideEffectClass: workspace-write)
- [x] 1.7 Convert /opsx:ff to bridged command with structuredExecutor (sideEffectClass: workspace-write)
- [x] 1.8 Convert /opsx:apply to bridged command with structuredExecutor (sideEffectClass: read)
- [x] 1.9 Write regression tests for bridged OpenSpec commands in extensions/openspec/bridge.test.ts

## 2. Register interactive-only commands with agentCallable: false
<!-- skills: typescript -->

- [x] 2.1 Register /dashboard with bridge (agentCallable: false) so it returns structured refusal instead of opaque "not registered"
- [x] 2.2 Register /dash with bridge (agentCallable: false)
- [x] 2.3 Wire cleave /assess to use getSharedBridge() instead of createSlashCommandBridge()

## 3. Verify side-effect metadata and interactive UX preservation
<!-- specs: harness/slash-command-bridge -->
<!-- skills: typescript -->

- [x] 3.1 Verify read-only commands (opsx:status, opsx:verify) declare sideEffectClass: read
- [x] 3.2 Verify write commands (opsx:propose, opsx:ff, opsx:archive) declare sideEffectClass: workspace-write
- [x] 3.3 Verify interactive /opsx:status and /opsx:verify render from structuredExecutor result
- [x] 3.4 Run full test suite (npm run check) — 1295 tests pass, 0 fail
