# Contribution loading - Delta Spec

## ADDED Requirements

### Requirement: Scope loading distinguishes absence from failure

Each attempted skills, plugins, or extensions scope MUST publish typed loading
health with its root and outcome. A failed scope MUST preserve its error category
and cause chain and MUST NOT be described as a healthy empty inventory.

#### Scenario: Missing directory
Given an otherwise valid installation and a missing contribution directory
When discovery attempts that scope
Then its outcome is absent without a failure notice

#### Scenario: Installation mismatch
Given a contribution directory whose maintenance state belongs to another home identity
When guarded discovery rejects the scope
Then its outcome is blocked with the maintenance error and actual root
And the loader does not execute rejected contributions

#### Scenario: Corrupted scope alongside healthy scope
Given a malformed user scope and a valid independent project scope
When discovery attempts both scopes
Then the user failure remains visible and the project contributions remain loaded

### Requirement: Contribution loading health is shared operator state

The existing status surface MUST expose scope outcomes and retained failure details
for TUI, CLI, and ACP consumers. Interactive startup MUST present one compact
notice when contribution scopes are blocked without dumping unrelated catalogs.

#### Scenario: Repeated startup status
Given three blocked scopes and repeated equivalent HarnessStatus events
When the interactive surface presents startup state
Then one contribution-health notice is shown
And the status command lists each blocked root and cause

#### Scenario: Native inline attachment already published
Given the inline terminal has published its initial session attachment notification
When a later status event reports blocked contribution scopes
Then the warning is appended as a new native publication
And repeated equivalent status events do not duplicate it

### Requirement: Recovery replaces stale failure state

A successful reload MUST replace the prior outcome for that scope while retaining
other scope outcomes. A requested or failed reload MUST NOT clear a failure early.

#### Scenario: Recovered directory
Given a previously blocked scope whose underlying problem has been corrected
When guarded discovery successfully reloads it
Then its outcome is loaded or absent as observed
And no stale blocked notice remains for that scope
