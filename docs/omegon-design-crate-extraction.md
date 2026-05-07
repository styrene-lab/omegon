+++
id = "6b79b41b-a8a8-4882-8000-b141bbd44470"
kind = "document"
title = "Extract design tree core into omegon-design crate"
status = "exploring"
tags = ["architecture", "crate", "design-tree", "task-management"]
aliases = ["omegon-design-crate-extraction"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
dependencies = []
issue_type = "feature"
open_questions = ["What is the stable crate boundary: markdown/frontmatter parsing plus node graph model only, or also query/filter/history operations and mutation helpers? Extracting too early will freeze the wrong API.", "Which dependencies are acceptable in the extracted crate: serde only, or also chrono/git helpers/frontmatter parsing utilities currently embedded in omegon? The portability goal is underspecified.", "What compatibility contract must the crate preserve for sovereign multi-repo PM: file-layout conventions only, or also tool schemas and lifecycle semantics? Without this, extraction can succeed mechanically but fail the downstream reuse goal."]
parent = "git-native-task-management"
priority = "1"
related = []
+++

# Extract design tree core into omegon-design crate

## Overview

Split the reusable design tree core out of omegon's lifecycle module into a standalone crate. Move types, markdown store/parsing, doctor/audit, filtering, history, and index logic into `core/crates/omegon-design/`. Keep agent-specific context injection and tool registration in the omegon binary.

## Decisions

### Decision: Crate extraction follows stabilization of the single-repo node/query model instead of leading it

**Status:** decided

**Rationale:** The extraction is valuable, but only after the storage/query semantics are proven by the single-repo task-management work. Otherwise the project will export internal churn as a public crate API and pay migration cost twice.

## Open Questions

- What is the stable crate boundary: markdown/frontmatter parsing plus node graph model only, or also query/filter/history operations and mutation helpers? Extracting too early will freeze the wrong API.
- Which dependencies are acceptable in the extracted crate: serde only, or also chrono/git helpers/frontmatter parsing utilities currently embedded in omegon? The portability goal is underspecified.
- What compatibility contract must the crate preserve for sovereign multi-repo PM: file-layout conventions only, or also tool schemas and lifecycle semantics? Without this, extraction can succeed mechanically but fail the downstream reuse goal.
