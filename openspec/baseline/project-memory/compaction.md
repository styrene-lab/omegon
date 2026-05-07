+++
id = "063232d9-9294-4209-8ce3-6a38b87b7767"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# project-memory/compaction

### Requirement: Normal compaction must not silently prefer heavy local inference

Routine context compaction must avoid local-first behavior when the available local chat models are materially high-latency. Default compaction behavior should prefer non-local routing for normal sessions and reserve local compaction for explicit opt-in or fallback after cloud failure.

#### Scenario: Default compaction does not intercept with local-first policy
Given Omegon is using the default project-memory configuration
When a session compaction is initiated during a normal session
Then the project-memory extension does not require local-first compaction by default
And cloud or provider-routed compaction remains the first attempted path unless a retry or explicit local policy applies

#### Scenario: Default effort tiers avoid local compaction for normal work
Given the operator is using a normal effort tier intended for day-to-day work
When compaction policy is resolved from the effort tier configuration
Then the compaction tier is not "local" by default for those normal work tiers
And heavy local inference is not silently selected for compaction

#### Scenario: Local compaction remains available as a recovery path
Given cloud compaction fails or the operator explicitly requests local compaction behavior
When the extension resolves its compaction fallback path
Then a local model may still be used as a later fallback
And existing retry/recovery behavior remains available

### Requirement: Compaction summaries must sanitize ephemeral clipboard temp paths

Compaction input and generated summaries must avoid preserving transient clipboard file-system paths that add noise or become stale immediately after capture.

#### Scenario: pi-clipboard temp paths are redacted before local summarization
Given the compaction input contains a path matching a transient pi clipboard temp image file
When project-memory prepares local compaction text for summarization
Then the emitted prompt text does not include the original temp file path verbatim
And the path is replaced with a stable redacted placeholder indicating a clipboard image attachment

#### Scenario: Redaction preserves non-clipboard file references
Given the compaction input contains ordinary repository file paths and a transient pi clipboard temp path
When path sanitization is applied
Then ordinary repository file paths remain unchanged
And only the transient clipboard temp path is redacted
