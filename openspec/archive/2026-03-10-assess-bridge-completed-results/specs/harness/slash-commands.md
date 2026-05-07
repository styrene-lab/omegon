+++
id = "5d533509-dfca-45c1-92ad-034570b12adf"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# harness/slash-commands — Delta Spec

## ADDED Requirements

### Requirement: Bridged /assess spec returns a completed structured result
Tool and agent invocation of `/assess spec <change>` MUST return only after the assessment logic has produced a completed structured result for the requested change.

#### Scenario: bridged assess spec does not stop at kickoff preparation
Given a caller invokes bridged `/assess spec my-change`
And the implementation can complete the assessment in-band for tool usage
When the slash-command bridge returns the structured result envelope
Then the envelope summary and data describe the completed assessment outcome rather than a kickoff banner
And the result does not depend on a later follow-up turn to determine whether work passed, reopened, or remained ambiguous

### Requirement: Interactive /assess may remain follow-up driven without corrupting the bridge contract
Interactive operator use of `/assess` MAY continue to use follow-up prompting, but that behavior MUST NOT leak into the structured bridge contract used by tools and agents.

#### Scenario: interactive and bridged assess flows diverge safely
Given `/assess spec my-change` is invoked interactively by an operator
And the same command is invoked through the structured slash-command bridge
When both executions complete
Then the interactive path may emit follow-up guidance for the operator
And the bridged path still returns a completed structured assessment result in the initial response

### Requirement: Bridged assess lifecycle metadata is trustworthy for reconciliation
When bridged `/assess spec` returns lifecycle metadata, that metadata MUST correspond to the completed assessment result so callers can safely decide whether to run `reconcile_after_assess` with `pass`, `reopen`, or `ambiguous`.

#### Scenario: lifecycle outcome matches the completed bridged assessment
Given bridged `/assess spec my-change` returns lifecycle metadata
When a caller inspects the returned assessment outcome and lifecycle fields
Then they describe the completed assessment result for the current implementation snapshot
And they do not represent a placeholder or preparatory state

### Requirement: Bridged /assess preserves normalized invocation args
The slash-command bridge MUST preserve the full original tokenized invocation in `result.args` for bridged `/assess` commands, even when returning completed structured assessment data.

#### Scenario: bridged assess keeps full original args
Given a caller invokes bridged `/assess spec my-change`
When the bridge returns the normalized structured envelope
Then `result.args` equals `["spec", "my-change"]`
And any supplemental assessment metadata is carried in structured fields without rewriting the original args array
