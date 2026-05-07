+++
id = "b8668d8c-4c83-4b09-8107-910461250bd2"
kind = "document"
title = "Add task-management fields, filtering, and history to design nodes"
status = "exploring"
tags = ["design-tree", "task-management", "query", "metadata"]
aliases = ["design-tree-task-fields-and-filters"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
dependencies = []
issue_type = "feature"
open_questions = ["What is the canonical archive model for task-management metadata: reuse the existing archived node status only, or also keep a separate frontmatter `archived` boolean for filtering/history? The parent currently mentions both, which is contradictory.", "What is the filter contract on `design_tree(list)`: exact-match only for status/tag/assignee/milestone/type/priority, or does it also support include-archived, sort, and multi-value filters? Another agent cannot implement the query surface cleanly without a concrete schema.", "How should node history be surfaced: raw git-log records, normalized JSON events, or markdown text summaries? The parent says git-derived history, but the operator-facing tool/result shape is unspecified."]
parent = "git-native-task-management"
priority = "1"
related = []
+++

# Add task-management fields, filtering, and history to design nodes

## Overview

Extend DesignNode/frontmatter with task-management metadata: milestone, assignee, estimate, actual, due, archived. Add filtered list queries and git-derived node history so the design tree can serve as a practical issue/task system without changing its core document model.

## Decisions

### Decision: Task-management metadata extends the existing node model rather than introducing a parallel task document

**Status:** decided

**Rationale:** The parent already made the strategic call: design nodes remain the only source of truth. This child should therefore implement metadata as optional additions to DesignNode/frontmatter and query surfaces on top of the current storage model, not invent a second task object or sidecar database.

## Open Questions

- What is the canonical archive model for task-management metadata: reuse the existing archived node status only, or also keep a separate frontmatter `archived` boolean for filtering/history? The parent currently mentions both, which is contradictory.
- What is the filter contract on `design_tree(list)`: exact-match only for status/tag/assignee/milestone/type/priority, or does it also support include-archived, sort, and multi-value filters? Another agent cannot implement the query surface cleanly without a concrete schema.
- How should node history be surfaced: raw git-log records, normalized JSON events, or markdown text summaries? The parent says git-derived history, but the operator-facing tool/result shape is unspecified.
