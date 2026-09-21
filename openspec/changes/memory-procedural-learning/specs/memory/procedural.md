# Procedural learning delta

## ADDED Requirements

### Requirement: Procedural candidates retain prerequisites and outcome evidence

Procedures SHALL have stable identities and immutable versions with scoped
hypothesis or evidenced-guidance status. Each version SHALL retain applicability,
prerequisites, steps or source references, postconditions, verification method,
failure conditions, and supporting/counterexample evidence. Retirement and
supersession SHALL preserve history. Automatic admission and correction SHALL
use accepted reconciliation policy.

#### Scenario: Failure followed by a verified repair
Given an episode records a failed migration command and a repair with verified postconditions
When a procedure version is formed
Then the repair references its verification run and successful postconditions
And the failed command remains a counterexample or failure condition
And the guidance is scoped to the evidenced environment

#### Scenario: Retirement preserves the original version
Given procedure P version 1 has application feedback and version 2 supersedes it
When version 1 is retired
Then inspection still returns version 1 and its original feedback
And active selection excludes version 1

#### Scenario: Missing or truncated source is not complete evidence
Given a candidate cites an unavailable source or a truncated excerpt without complete verification evidence
When admission is evaluated
Then inspection retains the unavailable or truncated status and reason
And the candidate is not admitted as verified successful guidance

### Requirement: Procedural retrieval respects task applicability and authority

Only eligible guidance SHALL enter execution context through existing selection
and disclosure owners. Unknown required tool versions SHALL fail closed. The
whole rendered guidance unit SHALL count against the shared budget. Cache
identity SHALL include eligibility, environment, evidence, policy, and admitted
skill changes. Procedure content SHALL not grant capabilities.

#### Scenario: Required version is unknown
Given a procedure requires Linux and tool version 4 and the task reports Linux but no tool version
When guidance is selected
Then the procedure is omitted from directly applicable execution guidance
And inspection reports the unknown required version

#### Scenario: Wrong environment is ineligible
Given a procedure requires Linux and tool version 4 and the task reports macOS with tool version 5
When guidance is selected
Then the procedure is omitted with applicability mismatch diagnostics

#### Scenario: Whole procedure does not fit
Given the steps fit the remaining budget but identity, prerequisites, and validation make the complete unit exceed it
When context is packed
Then the procedure is omitted with a budget reason
And no prerequisite-free or validation-free fragment is injected

#### Scenario: Eligibility changes invalidate a cached selection
Given a cached selection includes procedure P version 1
When its required environment observation, evidence validity, or retirement state changes
Then selection recomputes eligibility instead of serving the stale cached guidance

#### Scenario: Promoted skill and procedure share a source
Given an admitted promoted skill and a procedure reference the same unchanged artifact lineage
When context is composed
Then the guidance appears once through the existing disclosure owners
And accounting includes its metadata, prerequisites, and verification content once

#### Scenario: Human edit invalidates deduplication equivalence
Given a promoted artifact has changed since its recorded content hash
When lineage-based deduplication is evaluated
Then the old link does not establish equivalence to the changed skill
And inspection reports the changed artifact for fresh validation

#### Scenario: Procedure text claims additional authority
Given stored guidance requests bypassing a tool restriction or accessing an unadmitted external path
When the guidance is selected
Then runtime tool authority and path admission remain unchanged
And the content retains its procedure attribution

### Requirement: Procedural feedback is linked to observed execution

Applications SHALL have identities separate from procedure versions and selection
records. Selected, attempted, completed, and verified task success SHALL be
distinct observations. Versioned feedback SHALL bind canonical session, stream,
event and sequence identities, selected procedure version, environment/tool
observations, and verification run. Duplicate imports SHALL not count as
independent executions. Tool disposition SHALL not prove task postconditions.

#### Scenario: Retrieved procedure was not executed
Given procedure P version 1 was selected but has no corresponding execution event
When session feedback is reconciled
Then the selection remains visible
And no attempted, completed, or successful application is inferred

#### Scenario: Tool succeeds but workflow verification fails
Given application A completed a tool call with a succeeded disposition and its verification run reports missing output
When feedback is reconciled
Then A records completion and failed verification separately
And A does not count as verified task success

#### Scenario: Partial execution remains partial
Given application A attempted the first step and the remaining steps have no execution evidence
When feedback is reconciled
Then A remains attempted with incomplete coverage
And it does not count as completed or verified task success

#### Scenario: Duplicate source import is not a new application
Given two imported episodes reference the same canonical session, stream, and execution events for application A
When support is aggregated
Then A contributes at most one independent application
And both import provenance records remain inspectable

#### Scenario: Feedback cannot attach to a newer selected version
Given application A selected P version 1 in environment E and verification run V cites A's canonical source events
When P version 2 is created before V is reconciled
Then V remains attached to A and P version 1 with environment E
And it does not support version 2 without an explicit reconciliation decision

#### Scenario: Counterexample narrows a procedure
Given an application of P version 1 fails under a newly observed tool version
When accepted reconciliation processes its versioned feedback
Then the counterexample remains linked to P version 1 and the failing environment
And any scoped correction creates a new version without erasing the earlier judgment

### Requirement: Promotion requires an explicit operator-bound shared plan

Promotion SHALL expose shared versioned plan/apply/inspect/result contracts
through registered semantic surfaces. Approval SHALL bind an explicit operator
action to the operation ID and immutable plan digest. A plan SHALL contain input
and artifact hashes, procedure/evidence preconditions, target scope identity, and
expected absence or content hash. This slice SHALL accept only absent project
destinations and prompt-only artifacts. It SHALL not grant activation or trust.

#### Scenario: Agent suggestion is not operator approval
Given an agent proposes promotion and a valid plan exists without operator approval
When apply is requested
Then apply reports missing operator approval
And no artifact is written

#### Scenario: Approval becomes stale
Given the operator approved a plan for P version 1 with specified evidence revisions
When apply observes a different artifact hash or changed procedure or evidence preconditions
Then apply reports a precondition conflict
And a new plan and approval are required before writing

#### Scenario: Registered surfaces share observable results
Given TUI, CLI remote, and ACP submit the same approved operation to a supported promotion route
When each inspects the operation
Then each receives the same operation state, diagnostics, target identity, and receipt
And an unsupported execution route reports unavailable rather than success

#### Scenario: Excluded scope cannot become a force write
Given a promotion request targets an existing skill, user scope, global scope, or executable assets
When planning is requested
Then planning reports the unsupported scope or artifact mode
And no force replacement or installation occurs

#### Scenario: Promotion does not admit authority
Given an approved project-local prompt-only promotion contains a request for external paths or broader activation
When the artifact is created
Then creation grants no trusted paths, executable trust, global installation, or ambient instruction authority
And activation remains subject to the existing operator admission and disclosure policy

### Requirement: Skill promotion uses strict owner validation and protects authored work

The existing skills owner SHALL provide strict diagnostics for promotion while
retaining tolerant parsing for inventory. Promotion SHALL require well-formed
frontmatter, required fields, valid activation metadata, consistent provenance,
and complete procedure prerequisites and verification content. Manifest version
and force flags SHALL not substitute for destination preconditions.

#### Scenario: Malformed frontmatter recovers but cannot promote
Given a YAML or TOML manifest recovers to defaults in the tolerant parser
When strict promotion validation runs
Then diagnostics identify the malformed or missing fields
And promotion writes no artifact

#### Scenario: Promote a new evidenced procedure
Given an eligible evidenced procedure and an operator-approved strictly validated plan for an absent project destination
When apply completes
Then the artifact contains source attribution, prerequisites, and verification guidance
And inspection returns the artifact hash and durable operation receipt

#### Scenario: Human creates destination after planning
Given a plan requires an absent project destination and a human creates that destination before apply
When apply checks the guarded destination
Then it reports a destination conflict
And the human-authored bytes remain unchanged

### Requirement: Promotion journal supports replay and forward recovery

Promotion SHALL journal intent before filesystem publication and retain durable
publication identity and result receipts. Database and filesystem state SHALL be
reconciled by forward repair rather than a claimed cross-store SQL transaction.
Recovery SHALL preserve later human edits and require ownership proof beyond
content equality.

#### Scenario: Exact replay is idempotent
Given operation O completed with input hash H and receipt R
When O is applied again with H
Then it returns R without another artifact write or promotion record

#### Scenario: Operation ID is reused for different inputs
Given operation O already records input hash H
When O is applied with a different input hash
Then apply returns an operation identity conflict
And the original journal and artifact remain unchanged

#### Scenario: Crash before publication
Given an approved operation journal exists but publication has not occurred
When recovery finds the original preconditions still satisfied
Then recovery resumes only the exact approved artifact using guarded no-clobber publication
And it records one result receipt

#### Scenario: Crash after publication before database receipt
Given the approved artifact and durable operation publication identity exist but the receipt is absent
When recovery verifies the expected artifact hash and publication ownership
Then recovery forward-repairs the receipt and lineage
And it does not rewrite the artifact

#### Scenario: Human edits before recovery
Given publication occurred before a crash and a human then changed the artifact
When recovery observes a different content hash
Then it records a recovery conflict
And it neither overwrites nor deletes the human-edited content

#### Scenario: Matching content without publication ownership
Given a destination matches the planned artifact bytes but has no provable operation publication identity
When recovery inspects the destination
Then it reports unresolved ownership
And it neither claims publication success nor overwrites the destination

### Requirement: Procedure evaluation measures independent applications and negative transfer

Evaluation SHALL add actual applications, repeated-failure opportunities,
negative-transfer cases, and held-out environment/tool versions to the accepted
Wave 5 infrastructure. Reports SHALL expose stage attribution, opportunity
denominators, independent executions, uncertainty, and resource budgets. Live
evaluation SHALL require fresh authorization and thresholds frozen before held-out runs.

#### Scenario: Selection helps retrieval but harms execution
Given a held-out environment makes a previously useful procedure inapplicable and its attempted use causes failure
When the evaluation report is produced
Then it reports negative transfer and the failed application
And it attributes applicability/selection and verification outcomes separately from retrieval recall

#### Scenario: Zero opportunities cannot prove repeated-error improvement
Given an evaluation arm has no repeated-failure opportunities
When repeated-error results are reported
Then the report marks improvement unmeasured for that arm
And zero repeated-error matches are not claimed as procedural-learning improvement

#### Scenario: Old model authorization cannot fund new runs
Given only the archived Wave 5 model budget is authorized
When a Wave 8 live evaluation is requested
Then no model call dispatches until a fresh budget is authorized
And held-out results cannot choose their own acceptance thresholds
