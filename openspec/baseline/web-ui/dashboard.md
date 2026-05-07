+++
id = "32b10993-784c-49a5-94fa-94a929c475f1"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# web-ui/dashboard

### Requirement: Dashboard cleanup metadata is available to backend and web UI consumers

The metadata that the dashboard cleanup relies on for pinned operator state MUST also be available through the backend/web UI state contract, so TUI and web consumers observe the same dashboard-facing truth.

#### Scenario: backend snapshot exposes pinned operator metadata inputs
Given the dashboard cleanup introduces a pinned operator metadata block in raised mode
When the backend builds its control-plane dashboard snapshot
Then the snapshot includes the metadata needed to represent context/model/thinking and memory-oriented operator state
And web UI consumers do not need to reverse-engineer those values from unrelated footer text

#### Scenario: recovery actionability is represented structurally for web consumers
Given recovery rendering becomes conditional on actionability in the dashboard cleanup
When the backend exposes dashboard state to the web UI
Then the recovery snapshot includes enough structured information for web consumers to distinguish actionable from non-actionable recovery
And the web UI can make presentation choices consistent with the TUI dashboard
