# Inference Runtime Model — Delta Spec

## ADDED Requirements

### Requirement: Callable endpoints are vendor-neutral

The system SHALL represent a callable endpoint without requiring a vendor or administrative integration parent. An endpoint MAY belong to a neutral group used for ownership, credentials, or policy projection.

#### Scenario: Standalone private endpoint
Given a statically configured private endpoint using an understood adapter
And no administrative integration or producer identity is configured
When the inventory snapshot is built
Then the endpoint and its offerings are valid and callable
And no vendor-specific core record is synthesized

### Requirement: Adapter and transport are independent contracts

The system SHALL represent inference protocol behavior through an adapter identity and connectivity through a transport specification. Core validation SHALL reject transports missing required transport fields without coupling transport kind to a vendor.

#### Scenario: Same adapter over distinct HTTP endpoints
Given two endpoints use the same chat-completions adapter
And each has a different HTTP base URL and authentication reference
When the inventory is built
Then both endpoints share adapter semantics
And retain independent transport and credential policy metadata

#### Scenario: Local process endpoint
Given an endpoint uses an understood adapter over a local-process transport
When the inventory is built
Then it does not require an HTTP base URL
And its transport remains distinguishable from a remote endpoint for policy filtering

### Requirement: Policy attributes are composable

The system SHALL represent locality, operator, trust, and cost attributes independently rather than deriving them from a closed provider execution class.

#### Scenario: Private broker endpoint
Given an endpoint is remote, organization-operated, private-network scoped, and brokered
When policy metadata is inspected
Then each attribute is independently retained
And adding a new attribute value does not require a new core endpoint type

### Requirement: Extension metadata is opaque to core routing

The system SHALL preserve namespaced extension metadata on endpoints and offerings. Core compatibility filtering SHALL NOT interpret extension metadata.

#### Scenario: Connector metadata round-trips
Given a connector emits a namespaced deployment reference
When its inventory layer is merged
Then the metadata remains available in the active snapshot
And compatibility results are identical with or without that metadata

### Requirement: Bootstrap registry projects into the neutral model

The embedded model registry SHALL project endpoints into adapter, transport, optional grouping, and offering records without changing existing registry consumers.

#### Scenario: Existing registry remains valid bootstrap data
Given the shipped embedded model registry
When it is projected into a neutral inventory layer
Then the resulting snapshot validates
And every projected offering references an existing endpoint
And no runtime execution consumer is changed by this refactor
