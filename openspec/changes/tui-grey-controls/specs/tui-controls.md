# TUI controls — Delta Spec

## ADDED Requirements

### Requirement: Neutral control hierarchy
The default TUI SHALL distinguish control surfaces through shared, overridable
grey theme roles while retaining the terminal-default conversation canvas.

#### Scenario: Open a menu
Given the default theme in inline or fullscreen presentation
When the operator opens a connection or settings menu
Then the panel has a neutral background and readable foreground
And the selected row has a different background
And titles, descriptions, and hints use distinct foreground roles

#### Scenario: Move the selection
Given a menu or selector with multiple options
When the operator moves to the next option
Then the selection background moves to that option
And the previous option returns to the panel background
And the treatment survives narrow-width rendering and final-frame cleanup

#### Scenario: Composer and command fixtures
Given an empty composer
When the operator types a slash command
Then typed text replaces the subdued placeholder
And suggestions use the panel hierarchy
And command panels and prompts use the same surface and text roles

#### Scenario: Preserve conversation styling
Given the default theme
When a frame containing controls and conversation is finalized
Then the conversation canvas retains terminal-default colors
And existing Markdown modifiers and signal colors are preserved
And unsupported legacy fixed colors are removed
