# Inference Inventory — Delta Spec

## ADDED Requirements

### Requirement: Runtime inventory models heterogeneous inference estates

The system SHALL represent provider integrations, callable endpoint deployments, and model offerings as distinct identities. An offering MAY omit conceptual-model identity and quality grades while remaining usable when its inference interface and modalities are understood.

#### Scenario: Internal ungraded model is inventoried
Given a configured OpenAI-compatible endpoint deployment with an internal conversational model
And the model has no public benchmark grade or reviewed conceptual-model mapping
When the inventory snapshot is built
Then the deployment and offering are present with stable route identity
And the offering is marked ungraded rather than unsupported
And its configured inference interface and modalities are retained

#### Scenario: Heterogeneous endpoint offerings remain distinct
Given one provider integration exposes conversational, image-generation, and embedding deployments
When the inventory snapshot is built
Then each deployment and offering has independent protocol, modality, context, capability, and evidence metadata
And no provider-wide grade is used as offering-level fitness evidence

### Requirement: Inventory layers merge deterministically with provenance

The system SHALL compose embedded bootstrap, organization, user, project, session, discovery, and probe layers in deterministic precedence order. Fields SHALL retain their source provenance, and invalid candidates SHALL NOT replace the active last-known-good snapshot.

#### Scenario: Higher-precedence layer overrides one field
Given an embedded offering defines a context limit and capability set
And a project layer overrides only the context limit
When the layers are merged
Then the project context limit is active with project provenance
And untouched capabilities retain embedded provenance

#### Scenario: Invalid refresh preserves active generation
Given generation 4 is active
And a candidate refresh contains a dangling offering endpoint reference
When refresh is attempted
Then refresh fails with validation diagnostics
And generation 4 remains active without partial mutation

#### Scenario: Valid refresh activates atomically
Given generation 4 is active
And a complete valid candidate snapshot is built
When refresh succeeds
Then the complete candidate becomes generation 5 in one activation step
And readers observe either generation 4 or generation 5, never a partial merge

### Requirement: Routing compatibility precedes grade comparison

The system SHALL filter offerings by enabled state, inference interface, input/output modalities, required capabilities, and evidence confidence before applying optional capability-grade floors. An overall display tier SHALL NOT compensate for a failed hard requirement.

#### Scenario: Image route cannot satisfy text generation
Given an image-generation offering has an A display tier
And a conversational offering is ungraded
When text-generation compatibility is requested
Then the image-generation offering is rejected for incompatible output modality
And the conversational offering remains eligible if all hard requirements pass

#### Scenario: Autonomous routing excludes ungraded by default
Given an ungraded offering and a graded offering both satisfy hard compatibility requirements
When autonomous compatibility filtering uses the default policy
Then the ungraded offering is excluded
And explicit route selection can still select the ungraded offering

#### Scenario: Policy admits ungraded autonomous routing
Given an ungraded offering satisfies all hard requirements
And the routing policy explicitly allows ungraded offerings
When autonomous compatibility filtering runs
Then the offering remains eligible

### Requirement: Capability evidence is independent from quality grade

The system SHALL attach provenance and verification state to capability values independently of benchmark-derived quality grades.

#### Scenario: Probed tools support does not invent quality
Given a probe verifies tool-call support for an ungraded offering
When probe evidence is merged
Then tool-call support is marked verified with probe provenance
And no quality grade is synthesized from that verification
