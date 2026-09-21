# memory/evaluation - Baseline

### Requirement: Memory evaluation isolates evidence from answer labels

The runner SHALL expose only evidence available at the simulated query time to
the memory system. Gold answers and future events SHALL remain evaluator-owned.

#### Scenario: Correction arrives after the query
Given a fact at time T1 and a correction at time T3
When the runner evaluates a query at time T2
Then ingestion and retrieval cannot access the correction or gold answer
And the report identifies the evidence cutoff as T2

### Requirement: Deterministic memory cases run offline

The contract tier SHALL use injected clocks and controlled model/embedding outputs.

#### Scenario: Repeated offline evaluation
Given the same synthetic corpus and controlled dependency outputs
When the contract tier runs twice without credentials or network access
Then eligible evidence IDs and semantic pass/fail results are identical
And timing fields are excluded from deterministic equality checks

### Requirement: Memory evaluation attributes failures by stage

Reports SHALL distinguish formation, retrieval, selection, and downstream task
outcomes, including unavailable or incomplete execution.

#### Scenario: Correct evidence is dropped during packing
Given a source was retained and retrieved but omitted from the final context
When evaluation produces its report
Then it identifies a selection miss independently of formation and retrieval
And it records selected evidence IDs and the context budget

#### Scenario: Missing source requires abstention
Given a query whose answer is absent from the retained evidence
When evaluation checks a response claiming unsupported certainty
Then the abstention case fails
And the failure is not reported as a successful empty retrieval

### Requirement: Policy comparisons disclose quality and resource budgets

Comparison reports SHALL record workload revision, model configuration, per-case
outcomes, injected tokens, ingestion cost, query cost, and latency statistics.

#### Scenario: Compare candidate policy with baselines
Given no-memory, file-search, current-memory, and candidate-policy configurations
When the same held-out workload is evaluated
Then the report includes task success, repeated-error rate, stale-memory usage, and evidence recall for each configuration
And it identifies different resource budgets and incomplete runs
And acceptance thresholds were fixed before the held-out run
