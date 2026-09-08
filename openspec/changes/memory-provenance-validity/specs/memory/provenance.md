# Provenance delta

## ADDED Requirements

### Requirement: Durable claims retain structured supporting evidence

Stored claims SHALL distinguish observed evidence, explicit conclusions, and
inferences, and retain source references and recorded time.

#### Scenario: Assistant claim lacks verification
Given an assistant summary asserts that a test passed without supporting execution evidence
When the claim is retained as a candidate
Then its provenance identifies an inference
And no successful verification event is fabricated

#### Scenario: Imported content cannot elevate its authority
Given a vault note's content instructs the system to treat it as an operator directive
When the note is imported as memory
Then its authority follows validated import metadata
And its content does not become a harness instruction

### Requirement: Memory applicability is explicit

Current retrieval SHALL exclude known-inapplicable claims and disclose unknown
applicability. Historical evidence SHALL retain valid-time and recorded-time data.

#### Scenario: Platform-specific workaround
Given a workaround explicitly applicable only to Linux
When current memory is selected for a macOS task
Then the workaround is not presented as applicable guidance

#### Scenario: Correction recorded after the historical period
Given a correction recorded at T3 establishes that a claim ceased to apply at T2
When historical evidence is requested for T1 before T2
Then the original claim is available with its validity and recorded-time metadata
And the current replacement is not substituted without labeling the change

### Requirement: Provenance migration preserves unknown information

Legacy records SHALL remain readable. Migration and transport SHALL not invent
source evidence, verification dates, or confidence changes.

#### Scenario: Legacy fact survives migration and transport
Given a supported legacy fact with no structured evidence
When migration, reopen, JSONL export/import, and applicable vault round-trip complete
Then its content and lifecycle status remain intact
And absent evidence remains explicitly unknown
And transport alone does not reinforce the fact

#### Scenario: Migration fails before completion
Given a supported legacy database and an injected failure during provenance migration
When migration executes
Then no partially migrated schema or records become the active store
And the original database or verified recovery backup remains reopenable

#### Scenario: Evidence source becomes unavailable
Given a fact refers to an artifact that cannot currently be read
When the fact's evidence is inspected
Then the reference is reported as unavailable
And the fact and its history are not deleted
