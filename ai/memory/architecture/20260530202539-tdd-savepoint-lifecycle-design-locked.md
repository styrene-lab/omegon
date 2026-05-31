+++
id = "6d5e0a6e-99f8-4727-bb6d-48b0c9c024aa"
title = "TDD Savepoint Lifecycle design locked"
tags = []
aliases = []
source_format = "omegon_memory"
source_path = "omegon://memory/Architecture"
imported_at = "2026-05-30T20:25:39.510201Z"
imported_reference = true
kind = "memory_fact"
topic = "Architecture"

[publication]
enabled = false
visibility = "private"

+++

The TDD Savepoint Lifecycle design is locked in `design/tdd-savepoint-lifecycle.md` as a decided design node (`8d214819-082b-4742-8b4b-bcca1c528a9c`). Key decision: implement a deterministic `omegon tdd watch` red→green kernel first, with OpenSpec/design/task attribution layered on top; raw runner events are source of truth and agents cannot author them. Defaults: lifecycle event only, optional structured commit, raw logs under `.omegon/lifecycle/savepoints/`, durable summaries projected into OpenSpec.
