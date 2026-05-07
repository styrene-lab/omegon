+++
id = "6fd6340a-adca-4f98-b7f6-8d6bdfaf875d"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# runtime-security — Delta Spec

## ADDED Requirements

### Requirement: Insecure bootstrap transports are explicitly degraded

When daemon mode exposes plain HTTP or raw WebSocket transports, Omegon must treat them as degraded/bootstrap transports rather than the preferred security posture.

#### Scenario: Local insecure transport is surfaced as degraded
Given an Omegon daemon exposes a plain HTTP or raw WebSocket listener
When the daemon reports its runtime/control-plane state
Then the transport is marked as degraded or warned
And the operator can distinguish it from the secure happy path

### Requirement: HTTPS and WSS are the preferred network transports

When Omegon exposes networked HTTP/WebSocket control or ingress surfaces, HTTPS and WSS are the preferred secure transports.

#### Scenario: Secure transport is reported as preferred
Given an Omegon daemon exposes an HTTPS or WSS listener
When runtime transport metadata is reported
Then the transport is marked as secure or preferred
And it is distinguished from insecure bootstrap listeners

### Requirement: Insecure remote listeners are not implicit defaults

Daemon mode must not silently normalize insecure remote-access listeners as the default deployment posture.

#### Scenario: Insecure non-loopback listener requires explicit posture
Given an operator configures a non-loopback plain HTTP or raw WebSocket listener
When the daemon starts
Then Omegon emits a warning that the transport is insecure
And the daemon reports that listener as degraded rather than preferred

### Requirement: Future secure transport path is preserved

The daemon/runtime contract must remain compatible with future Styrene identity-based RPC transport.

#### Scenario: Runtime transport model can represent future secure RPC
Given the daemon reports its control-plane transport metadata
When a future Styrene RPC transport is introduced
Then the transport model can represent that secure managed path without redefining daemon instance identity or event-ingress semantics
