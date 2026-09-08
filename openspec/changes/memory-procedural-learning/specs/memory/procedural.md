# Procedural learning delta

## ADDED Requirements

### Requirement: Procedural candidates retain prerequisites and outcome evidence

Learned workflows SHALL record applicability, steps or source references,
validation, failure conditions, and supporting/counterexample evidence.

#### Scenario: Failure followed by a verified repair
Given an episode records a failed migration command and a verified successful repair
When procedural candidates are formed
Then the repair references its successful verification evidence
And the failed command is retained as a failure condition or counterexample
And the procedure is not generalized beyond its evidenced applicability

### Requirement: Procedural retrieval respects task applicability and authority

Only applicable guidance SHALL enter current task context. Procedure content
SHALL not alter harness authority or tool capability decisions.

#### Scenario: Procedure requires another environment
Given a procedure requires Linux and a particular tool version
When memory is selected for a task lacking those prerequisites
Then it is not injected as directly applicable execution guidance

#### Scenario: Procedure text claims additional authority
Given a stored procedure instructs the agent to bypass a tool restriction
When the procedure is retrieved
Then it remains attributed memory content
And runtime tool authority is unchanged

### Requirement: Procedural feedback is linked to observed execution

Helpfulness and failure feedback SHALL reference an actual application and outcome,
not merely retrieval frequency.

#### Scenario: Retrieved procedure was not executed
Given a procedure was included in context but no corresponding execution occurred
When session feedback is reconciled
Then no successful-application evidence is recorded for that procedure

#### Scenario: Counterexample narrows a procedure
Given a previously useful procedure fails under a newly observed tool version
When its execution evidence is reconciled
Then the counterexample remains linked to the procedure version
And the system proposes a scoped correction or unresolved limitation rather than erasing the history

### Requirement: Skill promotion uses existing validation and protects authored work

An explicit promotion operation SHALL validate the generated artifact through the
existing skills owner and compare the destination version before writing.

#### Scenario: Promote a new evidenced procedure
Given an evidenced procedural candidate and an unused skill destination
When explicit promotion executes
Then a skill valid under the existing parser is created with source attribution
And repeated promotion of the same operation does not duplicate the artifact

#### Scenario: Destination changed before promotion
Given a promotion plan captured a skill destination version that a human subsequently edited
When promotion executes
Then it reports a version conflict
And the human-authored content remains unchanged
