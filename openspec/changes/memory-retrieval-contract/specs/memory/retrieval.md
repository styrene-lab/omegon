# Retrieval delta

## ADDED Requirements

### Requirement: Historical retrieval uses explicit statuses

Search SHALL distinguish current knowledge from historical evidence. Archive
search SHALL include archived, dormant, and superseded matches with status labels.

#### Scenario: Archived match is discoverable
Given an archived fact and an unrelated active fact in the selected mind
When memory_search_archive searches the archived fact's distinctive terms
Then the archived fact is returned with its status
And the active fact is not presented as archived evidence

#### Scenario: Old low-salience evidence is searchable
Given a dormant fact excluded from current ambient retrieval
When a historical query matches that fact
Then the fact remains discoverable without the ambient confidence floor
And the read does not reinforce or reactivate it

### Requirement: Search filters apply before candidate limits

Section, mind, and status eligibility SHALL apply to all retrieval channels before
their result limits and to every expanded neighbor.

#### Scenario: Section filter survives hybrid retrieval
Given many Architecture matches and a matching Constraints fact
When memory_recall requests Constraints with a result limit of one
Then it returns the Constraints fact
And excluded Architecture seeds or neighbors cannot consume the result slot

### Requirement: Vector queries identify their embedding space

Vector comparison SHALL require compatible model/revision, preprocessing, and
dimension identity. Optional incompatibility SHALL leave lexical recall usable.

#### Scenario: Equal dimensions from different models
Given stored vectors from model A and a same-dimensional query from model B
When hybrid retrieval executes
Then it does not compare those vectors
And it returns lexical results with a typed embedding-incompatibility indication

#### Scenario: Legacy vector needs repair
Given a stored vector without a verifiable embedding-space identity
When the index repair operation reembeds its unchanged fact using the selected space
Then subsequent compatible queries may use the new vector
And replaying the completed operation does not create duplicate vectors

### Requirement: Retrieval scores identify their meaning

Results SHALL distinguish lexical, cosine, fusion, and graph scores without
presenting them as calibrated truth confidence or a universal match percentage.

#### Scenario: One fact participates in both channels
Given a fact returned by lexical and vector search
When recall renders the fused result
Then the result identifies its fusion score and available channel evidence
And it does not label the lexical score as a confidence percentage

### Requirement: Graph expansion preserves evidence eligibility

Expansion SHALL obey the same current/historical eligibility as seed retrieval,
respect relationship meaning, and use bounded deterministic traversal.

#### Scenario: Ineligible neighbor cannot bypass current filtering
Given a current seed linked to a below-floor neighbor and a superseded neighbor
When current retrieval expands the seed
Then neither neighbor is promoted as current supporting knowledge

#### Scenario: Contradiction is surfaced as a conflict
Given two eligible facts connected by a contradiction relationship
When retrieval expands the first fact
Then the second is labeled as conflicting evidence
And the relationship is not counted as corroboration
