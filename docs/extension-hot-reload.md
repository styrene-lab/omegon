---
id: extension-hot-reload
title: "Extension Hot-Reload (Development)"
status: seed
tags: [extensions, development, iteration]
open_questions: []
dependencies: []
related: []
---

# Extension Hot-Reload (Development)

## Overview

Allow developers to iterate on extensions without restarting Omegon TUI. Watch extension directory for changes, detect new binary or manifest modifications, gracefully shut down old process, spawn new process, re-register widgets. Useful for development. Can be feature-gated or require explicit command.
