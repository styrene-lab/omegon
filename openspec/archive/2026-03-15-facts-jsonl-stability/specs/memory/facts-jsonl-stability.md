+++
id = "a7700ffe-37a1-4ffc-9992-85ead00c3781"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# facts.jsonl stability

## Requirement: Reinforcement-only activity does not churn tracked fact transport
The tracked `.pi/memory/facts.jsonl` export must remain byte-stable when only runtime reinforcement metadata changes.

### Scenario: Reinforcement-only changes leave fact export unchanged
- **Given** a fact store whose durable facts are unchanged
- **And** one or more facts have only been reinforced locally
- **When** `exportToJsonl()` runs before and after that reinforcement-only activity
- **Then** the exported fact JSONL bytes are identical

## Requirement: Durable memory changes still appear in tracked transport
Durable knowledge changes must still propagate through the git-tracked JSONL export.

### Scenario: Adding durable knowledge changes the export
- **Given** an existing fact export snapshot
- **When** a new fact, supersession, or other durable transport-visible record is added
- **And** `exportToJsonl()` runs again
- **Then** the exported JSONL changes to reflect that durable knowledge change

## Requirement: Legacy richer JSONL remains importable
The importer must continue to accept older JSONL lines that include volatile runtime metadata fields.

### Scenario: Legacy fact lines with scoring metadata still import
- **Given** a historical `facts.jsonl` snapshot containing fact records with fields such as `confidence`, `last_reinforced`, `reinforcement_count`, and `decay_rate`
- **When** `importFromJsonl()` ingests that snapshot
- **Then** the import succeeds without requiring migration
- **And** durable fact identity and dedup behavior are preserved
