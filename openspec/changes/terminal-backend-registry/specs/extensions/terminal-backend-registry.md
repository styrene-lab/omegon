# Terminal Backend Registry — Delta Spec

## ADDED Requirements

### Requirement: Terminal create uses backend registry

Omegon SHALL route `terminal.create@1` execution through a backend registry after HostAction policy validation and before process creation.

#### Scenario: Policy denial prevents backend execution
Given a native extension requests `terminal.create@1` with a command denied by manifest policy
When the HostAction pipeline evaluates the action
Then no terminal backend is selected
And no terminal backend executes.

#### Scenario: Background fallback remains available
Given no visual terminal backend is available
When an extension requests `terminal.create@1`
Then Omegon executes the action through the built-in portable PTY background backend
And the result backend is `portable_pty`.

### Requirement: Placement degradation is explicit

Omegon SHALL report structured warnings when the selected backend cannot satisfy the requested placement.

#### Scenario: Side pane request degrades to background
Given no visual backend is available
When an extension requests `placement = side_pane`
Then the result actual placement is `background_session`
And the result warnings explain that `side_pane` degraded to background PTY.

### Requirement: Visual backend is preferred when capable

Omegon SHALL prefer a backend that can satisfy the requested placement over the background fallback.

#### Scenario: Visual backend satisfies side pane
Given a registered visual backend can satisfy `side_pane`
When an extension requests `placement = side_pane`
Then the selected backend is the visual backend
And the result actual placement is `side_pane`
And no degradation warning is emitted.
