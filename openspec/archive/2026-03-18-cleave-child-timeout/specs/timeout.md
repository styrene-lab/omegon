+++
id = "3e010f23-b1f3-4b50-afca-78ac2dc67e1a"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# timeout — Delta Spec

## ADDED Requirements

### Requirement: Idle timeout kills stalled children

Cleave children must be killed if no activity (stderr output in native/Rust path, RPC events in TS resume path) arrives within a configurable idle window. Default 3 minutes. Resets on every activity event.

#### Scenario: Child stalls with no activity

Given a cleave child is running with idle timeout of 180 seconds
When no activity event arrives for 180 seconds
Then the child process is killed
And the kill reason indicates idle timeout

#### Scenario: Active child keeps resetting idle timer

Given a cleave child is running with idle timeout of 180 seconds
When activity events arrive every 60 seconds
Then the idle timer resets on each event
And the child is not killed by idle timeout

### Requirement: Wall-clock timeout reduced to 15 minutes default

The hard wall-clock cap is reduced from 2 hours to 15 minutes. Configurable via cleave_run parameters. Acts as a hard backstop regardless of child activity.

#### Scenario: Wall-clock timeout fires at 15 minutes

Given a cleave child is running with default timeout settings
When the child has been running for 15 minutes
Then the child process is killed regardless of activity

### Requirement: idle_timeout_ms is configurable via cleave_run

The cleave_run tool accepts an optional idle_timeout_ms parameter to override the default 3-minute idle window. The value is threaded through to whichever dispatch backend is active.

#### Scenario: Custom idle timeout

Given a cleave_run call with idle_timeout_ms=300000
When a child is spawned
Then the child uses a 5-minute idle timeout instead of the default 3 minutes
