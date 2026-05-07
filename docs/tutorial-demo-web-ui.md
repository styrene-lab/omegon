+++
id = "574f5c0d-794f-4917-aec9-dc4950150ed6"
kind = "document"
title = "Tutorial demo project — sprint board web UI with visible bugs"
status = "implemented"
tags = ["tutorial", "demo", "web-ui", "0.15.1"]
aliases = ["tutorial-demo-web-ui"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
parent = "tutorial-demo-project"
+++

# Tutorial demo project — sprint board web UI with visible bugs

## Overview

Replace the Rust CLI config parser demo with a simple sprint board web app (HTML/CSS/JS) that has four independently-fixable bugs. The brokenness is visually obvious: wrong task counts, tasks in the wrong column, form that reloads the page, and data that vanishes on refresh. Four parallel cleave branches fix one bug each. After merge, the agent opens index.html in the browser — the user sees a live, working sprint board as evidence the lifecycle works.

## Decisions

### Decision: Four independent bugs, one function each — clean parallel branch merges

**Status:** decided
**Rationale:** Each bug lives in a separate named function in src/board.js: getTotalCount() (wrong selector), getTasksByStatus() (case mismatch), handleAddTask() (missing preventDefault), addTask() (missing saveTasks call). Branch 1 only touches getTotalCount, branch 2 only getTasksByStatus, etc. No overlapping hunks — merges are clean. The bugs are visually demonstrable: the count shows 12 when there are 6 tasks, the Done column is always empty, clicking Add reloads the page, and all data disappears on refresh.

## Open Questions

*No open questions.*
