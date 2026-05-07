+++
id = "6d7ae211-07cb-4245-84a3-15dda16f2a80"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# compaction-policy-hotfix — Tasks

## 1. Normal compaction must not silently prefer heavy local inference

- [x] 1.1 Default compaction does not intercept with local-first policy
- [x] 1.2 Default effort tiers avoid local compaction for normal work
- [x] 1.3 Local compaction remains available as a recovery path
- [x] 1.4 Write tests for Normal compaction must not silently prefer heavy local inference

## 2. Compaction summaries must sanitize ephemeral clipboard temp paths

- [x] 2.1 pi-clipboard temp paths are redacted before local summarization
- [x] 2.2 Redaction preserves non-clipboard file references
- [x] 2.3 Write tests for Compaction summaries must sanitize ephemeral clipboard temp paths
