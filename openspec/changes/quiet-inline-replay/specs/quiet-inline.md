# Quiet inline replay - Delta Spec

## ADDED Requirements

### Requirement: Inline sessions decline blocking splash replay

An inline session must answer `/splash` with a short notice without starting the
replay loop. The session base determines this policy even during borrowed
fullscreen inspection. Drafts, navigation, and runtime completion remain intact.

#### Scenario: Replay requested during streaming
Given an inline session with a held provider stream
When the operator submits `/splash`
Then the composer shows a notice and remains on the primary screen
And the stream can complete exactly once before an ordinary second submission

#### Scenario: Borrowed fullscreen is still an inline session
Given an inline session with a draft and a borrowed fullscreen inspector
When the splash command is dispatched
Then no replay is scheduled
And the draft and inspector remain intact
