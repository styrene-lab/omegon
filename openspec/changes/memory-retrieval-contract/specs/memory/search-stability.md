# Search stability delta

## MODIFIED Requirements

### Requirement: Malformed FTS-like user queries do not crash memory retrieval

Lexical search SHALL treat user terms as search data rather than executable FTS
syntax, preserving useful technical identifier tokens.

#### Scenario: Quotes and operators are input data
Given stored facts containing apostrophes, quoted names, paths, and hyphenated identifiers
When recall receives those terms with unmatched quotes or FTS-like operators
Then retrieval does not surface an FTS syntax error
And matching identifier-bearing facts remain discoverable

### Requirement: Operational storage failures remain observable

Storage and index operational failures SHALL remain distinguishable from an
empty match set. Optional vector unavailability SHALL be identified as degradation.

#### Scenario: Lexical storage fails during hybrid recall
Given the lexical storage operation fails for a reason unrelated to query syntax
When hybrid recall executes
Then the caller receives a typed operational failure
And the operation is not reported as no matching facts
