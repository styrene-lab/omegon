# terminal.create@1 — Delta Spec

## ADDED Requirements

### Requirement: argv-only terminal creation

Omegon SHALL launch `terminal.create@1` commands using argv command construction, not shell strings.

#### Scenario: Allowed command creates terminal session
Given a native extension manifest allows `terminal.create@1` and command `bookokrat`
When the extension requests `terminal.create@1` with command `bookokrat` and args `[/books/a.epub]`
Then Omegon creates or attempts a PTY-backed terminal session using argv
And the HostAction outcome contains `terminal_id`, `backend`, `actual_placement`, and `warnings` fields.

#### Scenario: Disallowed command denied before spawn
Given a manifest allows `terminal.create@1` but `allowed_commands = ["bookokrat"]`
When the action requests command `sh`
Then the HostAction outcome is `denied`
And no terminal process is spawned.

#### Scenario: Environment denied by default
Given the manifest does not list env keys in `allow_env`
When the action supplies any env var
Then the HostAction outcome is `denied`
And no terminal process is spawned.

#### Scenario: Allowed environment passes policy
Given the manifest lists `BOOKOKRAT_THEME` in `allow_env`
When the action supplies `BOOKOKRAT_THEME=dark`
Then policy accepts the env var for spawn.

#### Scenario: cwd roots enforced
Given a manifest allows cwd roots only under `${workspace}`
When the action requests a cwd outside the workspace
Then the HostAction outcome is `denied`
And no terminal process is spawned.

### Requirement: unsupported runtime behavior

Omegon SHALL return typed unsupported outcomes when terminal creation is unavailable.

#### Scenario: PTY backend unavailable
Given the PTY terminal backend cannot allocate a terminal
When the action requests `terminal.create@1`
Then the outcome status is `unsupported`
And the error code explains terminal backend unavailability.

### Requirement: placement and reuse semantics

Omegon SHALL treat placement as advisory unless marked required and SHALL namespace reuse keys by origin.

#### Scenario: Advisory placement degradation
Given requested placement is unsupported but not required
When the terminal can be opened elsewhere
Then the outcome is completed
And result warnings describe the degraded placement.

#### Scenario: Reuse key is scoped
Given two extensions request the same reuse key
When Omegon computes the terminal reuse identity
Then the identities differ by origin extension/session.
