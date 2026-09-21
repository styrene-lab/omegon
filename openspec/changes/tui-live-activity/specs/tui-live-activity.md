# Live activity — Delta Spec

## ADDED Requirements

### Requirement: Visible current action
The TUI SHALL show a compact, transient action area from existing runtime events.

#### Scenario: Model phases
Given an active turn with activity enabled
When provider, thinking, and response events arrive
Then the action area identifies Working, Thinking, and Responding respectively
And the Thinking label does not reveal the thinking payload

#### Scenario: Tool execution
Given an active turn
When a tool starts
Then the area shows Working with the tool name and bounded argument summary
And concurrent running tools are represented by a count
And tool completion removes the running indication

#### Scenario: Error and cancellation
Given a running tool
When it fails or cancellation begins
Then activity identifies failure or Canceling without retaining a false running state
And authoritative turn completion hides the area

### Requirement: Activity does not interrupt the response
Inline activity SHALL stay after the live response and before the composer, outside
the transcript publication stream. Both terminal layouts use the same phase source.

#### Scenario: Live text followed by activity
Given published response text and an unfinished live tail
When activity changes
Then no activity line appears between that published text and live tail
And the strip follows the live tail and precedes the composer
And native scrollback contains no transient action rows

#### Scenario: Narrow or hidden activity
Given a narrow or short terminal, or an explicit hidden activity preference
When the TUI draws
Then activity stays within the available rectangle or is hidden as requested
And input remains available
And tool strings cannot inject terminal controls or additional rows

#### Scenario: Authoritative completion and another turn
Given an active turn whose advisory AgentEnd event is absent
When an authoritative completed lifecycle or idle queue snapshot arrives
Then the action area clears
And a subsequent prompt can start a new turn with fresh activity
