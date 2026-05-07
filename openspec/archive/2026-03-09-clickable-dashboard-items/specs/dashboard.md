+++
id = "53225639-2135-4a00-ba00-d4303d9c4262"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# dashboard — Delta Spec

## ADDED Requirements

### Requirement: Clickable Design Tree dashboard items

Design Tree items rendered by the dashboard must expose clickable OSC 8 links when the underlying design document path is known.

#### Scenario: Design footer entry opens the design document

Given a Design Tree dashboard item with a known markdown file path
And mdserve is running for the project root
When the dashboard renders the Design Tree item in the footer or overlay
Then the item text is wrapped in an OSC 8 link
And the link target is the mdserve HTTP URL for that markdown file

#### Scenario: Design footer entry falls back to file URI

Given a Design Tree dashboard item with a known markdown file path
And mdserve is not running
When the dashboard renders the Design Tree item in the footer or overlay
Then the item text is wrapped in an OSC 8 link
And the link target is a file:// URI for that markdown file

### Requirement: Clickable OpenSpec dashboard items

Top-level OpenSpec change items rendered by the dashboard must expose clickable OSC 8 links when the change directory is known.

#### Scenario: OpenSpec change opens proposal by default

Given an OpenSpec dashboard change with a known change directory
And a proposal.md file exists in that change directory
When the dashboard renders the top-level OpenSpec change item
Then the change name is wrapped in an OSC 8 link
And the link target is the resolved URI for proposal.md in that change directory

#### Scenario: OpenSpec change stays plain when no proposal exists

Given an OpenSpec dashboard change without a proposal.md file
When the dashboard renders the top-level OpenSpec change item
Then the change name is rendered without an OSC 8 link

### Requirement: Shared URI resolver consistency

Dashboard links must use the same URI resolution rules as the view tool so markdown routes to mdserve when available and degrades gracefully otherwise.

#### Scenario: Dashboard link generation delegates to the shared resolver

Given the dashboard renders a clickable item for a known file path
When the dashboard computes the URI target
Then it uses the shared URI resolver module
And it passes the current mdserve port when available

## MODIFIED Requirements

### Requirement: Footer entries and overlay rows are clickable in the first slice

The first implementation slice includes clickable links in both footer summaries and overlay top-level rows for Design Tree and OpenSpec items.

#### Scenario: Footer and overlay both expose clickable items

Given the dashboard has Design Tree or OpenSpec items with known file targets
When the dashboard renders footer sections and overlay top-level rows
Then both surfaces render clickable OSC 8 links for those items

### Requirement: OpenSpec top-level rows default to proposal.md

The dashboard opens proposal.md for top-level OpenSpec change rows because it is the primary entry point for understanding the change.

#### Scenario: Top-level OpenSpec row prefers proposal.md over other artifacts

Given an OpenSpec change directory containing proposal.md, design.md, and tasks.md
When the dashboard renders the top-level change row
Then the clickable target resolves to proposal.md
And it does not resolve to design.md or tasks.md by default
