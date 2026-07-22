+++
title = "Operator-visible harness terminal sessions"
tags = ["tui","terminal","operator-experience"]
+++

+++
id = "a4ff068a-29e4-43ee-ae52-ff285ba751da"
kind = "design_node"

[data]
title = "Operator-visible harness terminal sessions"
status = "seed"
issue_type = "feature"
priority = 2
dependencies = []
open_questions = []
+++

## Overview

Background terminal sessions created through the agent harness are currently internal execution surfaces. The operator cannot attach to, view, or interact with them from the active Omegon TUI. Consequently, launching an interactive application such as `just run` in a harness terminal does not provide the operator with a usable local verification session.

The future feature should project harness-managed terminal sessions into an operator-visible surface with explicit attach, observe, input, detach, and terminate controls while preserving process ownership and permission boundaries.

## Current constraint

Until this feature exists, agents must not claim that an operator can use an interactive process launched via the harness terminal tool. For operator verification, provide the exact command for the operator to run in their own terminal, or use an existing operator-visible control surface.

## Open Questions

- [assumption] The first implementation should expose existing harness terminal sessions rather than introduce a second terminal-process runtime.
- What is the canonical operator surface: an in-TUI terminal pane, external terminal handoff, or both?
- How are stdin ownership and resize events transferred safely between the agent and operator?
- Should interactive sessions pause agent writes while operator-attached, or support explicit shared-control modes?
- What lifecycle and transcript evidence must remain visible after detach or termination?
