+++
id = "e255a65f-37a5-4f63-b526-2c859e715bd7"
kind = "document"
title = "File watcher — detect external changes during session"
status = "seed"
tags = ["ux", "filesystem", "ide-integration", "competitive"]
aliases = ["file-watcher"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
issue_type = "feature"
open_questions = []
priority = "2"
+++

# File watcher — detect external changes during session

## Overview

OpenCode has an experimental file watcher that detects external changes (IDE edits, git operations) during a session and notifies the agent. Without this, the agent's view of the filesystem can become stale if the operator edits files in their IDE while the agent is working. notify crate (Rust) provides cross-platform filesystem watching.

## Open Questions

*No open questions.*
