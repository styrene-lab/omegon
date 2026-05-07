+++
id = "c31b154d-28c3-478a-82a2-f542cfe06be1"
kind = "document"
title = "Structured commit tool — replaces bash git commit"
status = "implemented"
tags = ["rust", "git", "tools"]
aliases = ["commit-tool"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
parent = "git-harness-integration"
+++

# Structured commit tool — replaces bash git commit

## Overview

A first-class agent tool that replaces git commit via bash. Takes a message and optional scope. Consults RepoModel for dirty files, handles submodule two-level commits automatically, folds in pending lifecycle changes, and applies commit policy (conventional commit format validation). The agent calls commit(message) instead of bash(git add -A && git commit -m ...).

## Open Questions

*No open questions.*
