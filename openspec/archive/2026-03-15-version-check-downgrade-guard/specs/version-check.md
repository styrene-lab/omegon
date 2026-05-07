+++
id = "c4b64c4f-44c5-4d7a-852d-5f9c0d0d90ee"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# version-check — Delta Spec

## ADDED Requirements

### Requirement: Update notifications only surface newer versions

The interactive version checker must only notify the operator when the registry reports a version that is newer than the running build.

#### Scenario: Older registry version is ignored
Given the running build version is `0.58.1-cwilson613.1`
And the registry reports `0.57.1-cwilson613.2`
When interactive startup checks for updates
Then no update notification is shown

#### Scenario: Newer registry version is announced
Given the running build version is `0.58.1-cwilson613.1`
And the registry reports `0.58.1-cwilson613.2`
When interactive startup checks for updates
Then the update notification references `0.58.1-cwilson613.2`
