+++
id = "28a19735-2ce2-4ae0-905c-ad67f93c2f6b"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# provider-neutral-model-controls — Tasks

## 1. Operator-facing model controls describe capability tiers, not Anthropic-only products

- [x] 1.1 Slash-command help uses provider-neutral wording
- [x] 1.2 Tool help reflects provider-aware routing
- [x] 1.3 Write tests for Operator-facing model controls describe capability tiers, not Anthropic-only products

## 2. Sessions restore the last explicitly selected driver model

- [x] 2.1 Session start restores last selected model
- [x] 2.2 Missing persisted model falls back safely
- [x] 2.3 Only successful explicit switches are persisted
- [x] 2.4 Write tests for Sessions restore the last explicitly selected driver model
