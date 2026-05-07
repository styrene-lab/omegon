+++
id = "afffb1be-12e9-43e2-9911-e9bc4038222e"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# runtime — Delta Spec

## ADDED Requirements

### Requirement: Persistent daemon server mode

Omegon must support a long-running daemon/server mode that outlives individual interactive attachments.

#### Scenario: Start a persistent daemon
Given the operator starts Omegon in daemon/server mode
When startup completes successfully
Then Omegon exposes a stable instance identity for that process lifetime
And Omegon remains available for later attachment without requiring an active TUI session

### Requirement: Canonical local control plane remains IPC

Local first-party control of a daemon instance must use the native IPC contract rather than the embedded browser compatibility surface.

#### Scenario: Local client attaches to a running daemon
Given an Omegon daemon is already running locally
When Auspex or another first-party local client attaches
Then the attachment uses Omegon's native IPC control plane
And the daemon reports the same instance identity across repeated local attachments

### Requirement: Minimal HTTP ingress surface for health and events

Daemon mode must expose a minimal localhost HTTP surface for health checks and typed event submission.

#### Scenario: Health probe succeeds on a healthy daemon
Given an Omegon daemon is running and ready
When a local supervisor requests the health endpoint
Then the daemon returns a success response indicating readiness

#### Scenario: Typed event is submitted to the daemon
Given an Omegon daemon is running
When a caller submits a valid event envelope to the local event-ingress endpoint
Then the daemon acknowledges receipt
And the event is queued for processing by the daemon runtime

### Requirement: Typed daemon event envelope

Daemon event ingress must accept a structured event envelope that is generic enough to support future webhook, connector, and scheduler producers.

#### Scenario: Event envelope carries producer metadata
Given a caller submits an event envelope
When Omegon validates the request
Then the envelope includes an event id, source, trigger kind, and payload
And Omegon preserves that metadata for routing and observability

### Requirement: Shared runtime shape for Auspex-managed and standalone service instances

Persistent Omegon instances launched behind Auspex and standalone headless agent services must share the same daemon runtime model.

#### Scenario: Ownership metadata differs without changing runtime architecture
Given one daemon instance is launched behind Auspex and another is launched standalone
When their instance descriptors are compared
Then both expose the same daemon runtime/control-plane shape
And they differ only in ownership metadata or ingress policy
