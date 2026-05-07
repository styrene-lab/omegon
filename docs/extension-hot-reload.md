+++
id = "d6d13479-a20c-4586-a983-df1770a7e51d"
kind = "document"
title = "Extension Hot-Reload (Development)"
status = "seed"
tags = ["extensions", "development", "iteration"]
aliases = ["extension-hot-reload"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
dependencies = []
open_questions = []
related = []
+++

# Extension Hot-Reload (Development)

## Overview

Allow developers to iterate on extensions without restarting Omegon TUI. Watch extension directory for changes, detect new binary or manifest modifications, gracefully shut down old process, spawn new process, re-register widgets. Useful for development. Can be feature-gated or require explicit command.
