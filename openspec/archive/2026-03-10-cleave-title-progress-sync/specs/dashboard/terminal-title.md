+++
id = "e465be70-a4d0-491b-a615-73f837b0382e"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# dashboard/terminal-title — Delta Spec

## ADDED Requirements

### Requirement: Terminal title reflects live cleave child progress
When cleave child execution changes from pending to running or from running to done/failed, the terminal tab title must refresh so operators can see current progress counts without waiting for the overall phase to change.

#### Scenario: child progress updates refresh the terminal title
Given a cleave run is dispatching 3 children
When the dispatcher marks the first child running and later marks it done
Then dashboard update events are emitted for those child-progress transitions
And terminal-title consumers can refresh from shared cleave state immediately
And the terminal title can reflect counts such as 0/3, 1/3, and 2/3 as work progresses
