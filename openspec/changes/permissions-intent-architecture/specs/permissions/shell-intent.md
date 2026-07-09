# Shell Intent Extraction — Delta Spec

## ADDED Requirements

### Requirement: Shell preflight extracts filesystem intents

Bash and terminal preflight MUST extract filesystem intents before execution for known write-like shell patterns.

#### Scenario: Redirect target extracted as write intent
Given the command `echo x > /etc/example`
When shell preflight runs
Then it extracts a `Write` intent for `/etc/example`
And records the source as a shell redirect.

#### Scenario: Relative workspace redirect is allowed
Given the command `echo x > .omegon/example`
When shell preflight evaluates the extracted intent
Then the target resolves inside the workspace
And no permission prompt is required.

#### Scenario: Root-dot mkdir is suspicious
Given the command `mkdir -p /.omegon/runtime`
When shell preflight evaluates the extracted intent
Then the target resolves outside the workspace
And the diagnostic suggests `.omegon/runtime` as likely intended workspace-relative spelling.
