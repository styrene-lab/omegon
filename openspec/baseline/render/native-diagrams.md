+++
id = "3651f044-cc7b-4543-a6c6-36c18d943330"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# render/native-diagrams

### Requirement: Native diagrams are generated from constrained motif-based specs

Omegon must provide a native diagram backend for document-bound technical diagrams that accepts constrained structured specs instead of raw drawing geometry.

#### Scenario: Motif-based spec compiles to native scene output
Given an agent wants to generate a document-bound architecture diagram
When it uses the native diagram backend
Then it provides a constrained spec with a supported motif and structured nodes or segments
And the backend compiles that spec into deterministic geometry without requiring the agent to hand-author SVG coordinates

### Requirement: The native backend renders SVG directly and can rasterize to PNG without a browser runtime

The native backend must emit SVG as its primary output and support PNG export through a Node-native rasterization path.

#### Scenario: Native SVG render path avoids Playwright or Chromium
Given a valid native diagram spec
When the backend renders it
Then it writes an SVG artifact directly
And it can optionally produce a PNG artifact without using Playwright or Chromium

### Requirement: The native backend remains a sibling rendering path inside the render extension

The new backend must coexist with D2 and Excalidraw tooling so operators can choose the best output path per diagram type.

#### Scenario: Render extension exposes native diagram generation without removing existing tools
Given Omegon has existing D2 and Excalidraw render tools
When the native backend is added
Then the render extension registers a new native diagram tool alongside them
And the existing D2 and Excalidraw tools remain available

### Requirement: MVP scope stays narrow and deterministic

The first implementation must stay tightly scoped to a small set of document-oriented motifs and avoid editor-level complexity.

#### Scenario: Unsupported advanced behaviors remain out of scope for MVP
Given a request for arbitrary freeform drawing, full whiteboard editing semantics, or broad relation-language features
When the native backend MVP is used
Then the implementation focuses only on its supported motif-based diagram set
And it does not require general-purpose constraint solving or editor frameworks to operate
