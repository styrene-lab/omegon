+++
id = "50104e43-68a4-4491-9bf3-e35d2160cf80"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Memory: Episode Generation Reliability — Cloud fallback and guaranteed per-session narrative — Design Spec

## Scenarios

### Scenario 1 — Ollama unavailable at shutdown
Given Ollama is not running when /exit is called
When episode generation fires
Then the system attempts codex-spark, then haiku, in that order
And at least one model succeeds and produces a narrative episode
And the episode is stored before process exit

### Scenario 2 — All models fail
Given every model in the fallback chain times out or errors
When episode generation fires
Then a template episode is constructed from session telemetry (date, tool call count, files written, topics)
And the template episode is stored — no session goes unrecorded

### Scenario 3 — Episode generation stays within shutdown budget
Given the shutdownExtractionTimeout budget (15s)
When the fallback chain runs
Then individual model timeouts are allocated so they sum to fit within the budget
And shutdown is never blocked beyond the existing budget

### Scenario 4 — Successful Ollama path unchanged
Given Ollama is running and responsive
When episode generation fires
Then Ollama is tried first and used if successful
And the cloud fallback models are never called

## Falsifiability

- A session can end with zero episodes stored → design fails
- Episode generation extends shutdown beyond shutdownExtractionTimeout → design fails
- Template episode is missing date, tool call count, or files written → design fails

## Constraints

- Template episode must use only already-collected in-memory telemetry — zero additional I/O at construction time
- Fallback chain reuses the same model IDs as the compaction fallback chain
- Episode null/undefined output is a bug, never a valid codepath
