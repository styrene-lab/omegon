+++
id = "1bc483a5-6aa0-400e-8352-173af833a710"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Cleave child timeout and idle detection — Tasks

## 1. extensions/cleave/dispatcher.ts (modified)

- [x] 1.1 Define DEFAULT_CHILD_TIMEOUT_MS (15 min) and IDLE_TIMEOUT_MS (3 min) constants
- [x] 1.2 Add idle timer in spawnChildRpc() — reset on each RPC event, kill child when fired (TS resume path)

## 2. extensions/cleave/index.ts (modified)

- [x] 2.1 Expose idle_timeout_ms as optional cleave_run param
- [x] 2.2 Thread timeoutSecs and idleTimeoutSecs to native dispatch config

## 3. extensions/cleave/native-dispatch.ts (new)

- [x] 3.1 Pass --timeout and --idle-timeout CLI args to Rust omegon-agent cleave

## 4. core/crates/omegon/src/cleave/orchestrator.rs (new)

- [x] 4.1 Implement idle timeout via tokio::time::timeout on stderr line reads
- [x] 4.2 Implement wall-clock timeout via tokio::select with tokio::time::sleep
- [x] 4.3 Kill child immediately on idle/wall-clock timeout (kill_on_drop + explicit kill)
