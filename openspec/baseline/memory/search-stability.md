+++
id = "cc1ef366-c2df-42d9-b1b2-676c3fb4680a"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Memory Search Stability

### Requirement: Malformed FTS-like user queries do not crash memory retrieval
Memory search MUST tolerate user-entered apostrophes and FTS-like punctuation without surfacing syntax errors to the operator.

#### Scenario: Apostrophe-bearing search remains valid
- **Given** facts containing words with apostrophes
- **When** search is executed with a query like `user's auth`
- **Then** memory search returns matching facts
- **And** it does not raise an FTS syntax error

#### Scenario: Technical identifier search preserves useful recall
- **Given** facts containing path-like or identifier-like technical text such as `extensions/project-memory/factstore.ts` or `openai-codex`
- **When** memory search is executed with those technical query forms
- **Then** the generated FTS query preserves useful identifier tokens instead of destroying them into unusable fragments
- **And** matching facts remain discoverable

### Requirement: Operational storage failures remain observable
Memory search should be tolerant of malformed input, but it MUST NOT silently convert unrelated storage or FTS operational failures into empty results.

#### Scenario: Non-query operational failure is surfaced
- **Given** the underlying fact store encounters an operational failure unrelated to user query syntax
- **When** a memory search is executed
- **Then** the failure is surfaced to the caller rather than being silently converted into an empty result set
