# Runtime Inference Manifests — Delta Spec

## ADDED Requirements

### Requirement: Versioned declarative manifests

The system SHALL parse versioned TOML inference manifests into vendor-neutral inventory layers. Unknown schema versions SHALL be rejected.

#### Scenario: Project adds standalone endpoint
Given a project inference manifest declares an HTTP endpoint and text offering
And the endpoint uses an understood adapter
When runtime inventory is loaded
Then the endpoint and offering appear in the activated snapshot
And no provider group is required

#### Scenario: Unknown schema version
Given a manifest declares an unsupported schema version
When runtime inventory is loaded
Then reload fails with a schema diagnostic
And the active generation does not change

### Requirement: Deterministic scope precedence

The system SHALL compose embedded, organization, user, project, and session layers in that order. Fields absent from higher-precedence records SHALL retain lower-layer values and provenance.

#### Scenario: Session overrides enabled state only
Given a project manifest defines a complete endpoint
And a session manifest disables that endpoint without repeating its transport
When layers are loaded
Then the endpoint is disabled
And its transport retains project provenance

### Requirement: Optional and configured path behavior

The system SHALL ignore absent optional manifests. A path explicitly configured by the operator SHALL produce a read diagnostic when unreadable.

#### Scenario: Default user manifest absent
Given no default user manifest exists
When runtime inventory is loaded
Then loading continues without an error

#### Scenario: Explicit manifest unreadable
Given an explicitly configured manifest path cannot be read
When reload is requested
Then reload fails with a read diagnostic naming the path
And the active snapshot remains unchanged

### Requirement: Atomic last-known-good reload

The system SHALL parse and validate all candidate layers before replacing the active snapshot.

#### Scenario: One manifest is invalid
Given generation 4 is active
And one candidate manifest contains an invalid endpoint transport
When reload is requested
Then reload returns validation diagnostics
And generation 4 remains active with identical records

### Requirement: Redacted diagnostics

Reload diagnostics SHALL identify source scope, path, and failure phase without containing secret values.

#### Scenario: Secret value supplied as reference
Given a manifest puts a token-like value in `secret_refs`
When reload is requested
Then validation rejects the manifest
And the diagnostic does not reproduce the supplied value
