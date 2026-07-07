# Harness Guidance Observation — Delta Spec

## ADDED Requirements

### Requirement: Unified observation events

The harness guidance loop must normalize completed tool calls into semantic observation events before updating guidance state.

#### Scenario: Capability-catalog read tool records file evidence
Given a successful tool call whose tool definition has repo-inspection capability
And the tool call arguments include `path: "docs/a.md"`
When the conversation state updates from tool calls
Then the guidance intent records `docs/a.md` as read evidence
And the implementation does not require that tool name to be literally `read` or `understand`

#### Scenario: Broad search records search evidence without pretending to read a file
Given a successful broad repo-inspection tool call with no file path argument
When observations are normalized
Then a search observation is emitted
And no file-read path is added for that search alone

#### Scenario: Failed tool calls produce no positive evidence
Given a tool call result is marked as an error
When observations are normalized
Then no file-read, mutation, validation, or progress-boundary event is emitted for that failed call

### Requirement: Conservative bash observation classification

The harness guidance loop must classify common bash commands into observation events without treating arbitrary shell text as evidence.

#### Scenario: Bash file read records the referenced file
Given a successful bash command `sed -n '1,80p' core/crates/omegon/src/conversation.rs`
When observations are normalized
Then the guidance intent records `core/crates/omegon/src/conversation.rs` as read evidence
And the sed range argument is not treated as a file path

#### Scenario: Bash search records search evidence
Given a successful bash command `rg "OrientationChurn" core/crates/omegon/src docs`
When observations are normalized
Then a search observation is emitted
And the search does not add every search-root argument as a read file

#### Scenario: Bash validation records validation activity
Given a successful bash command `cargo test -p omegon pressure_behavior --locked`
When observations are normalized
Then a validation observation is emitted

#### Scenario: Bash commit records a progress boundary
Given a successful bash command `git commit -m 'fix: example'`
When observations are normalized
Then a progress-boundary observation is emitted
And outstanding modified-file guidance state is cleared

#### Scenario: Unknown bash program is opaque
Given a successful bash command `custom-tool --flag value`
When observations are normalized
Then no positive evidence event is emitted

## MODIFIED Requirements

### Requirement: Guidance state updates use normalized observations

Existing guidance state updates must consume normalized observation events instead of independently parsing raw tool calls with local name tables.

#### Scenario: Intent document no longer has a read-tool allowlist blind spot
Given a successful `view` tool call with `path: "README.md"`
When `IntentDocument::update_from_tools` runs
Then `README.md` is present in `files_read`

#### Scenario: Existing mutation tracking remains compatible
Given a successful edit-style mutation observation for `src/lib.rs`
When guidance state updates
Then `src/lib.rs` is present in `files_modified`

#### Scenario: Bash commit behavior remains compatible
Given `files_modified` is non-empty
And a successful bash commit command is observed
When guidance state updates
Then `files_modified` is empty
And commit nudging is reset
