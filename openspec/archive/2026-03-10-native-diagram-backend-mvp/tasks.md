+++
id = "8cc0eddb-f9cc-4150-b68f-58db5037249b"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Native SVG diagram backend MVP — Tasks

## 1. Native diagram pipeline in `extensions/render/native-diagrams/` <!-- specs: render/native-diagrams -->

- [x] 1.1 Add a constrained native diagram schema/parser for motif-based specs with nodes, edges, and optional panels.
- [x] 1.2 Add a small deterministic motif compiler for `pipeline`, `fanout`, and `panel-split` document layouts.
- [x] 1.3 Add a neutral scenegraph plus direct SVG serialization for native diagram output.
- [x] 1.4 Add Node-native PNG rasterization for generated SVG without Playwright or Chromium.

## 2. Render extension integration in `extensions/render/index.ts` <!-- specs: render/native-diagrams -->

- [x] 2.1 Add a `render_native_diagram` tool that accepts constrained JSON specs and writes SVG output.
- [x] 2.2 Optionally rasterize native SVG output to PNG and save artifacts in the shared visuals directory.
- [x] 2.3 Keep existing D2 and Excalidraw tools available alongside the new native backend.

## 3. Dependency and operator guidance <!-- specs: render/native-diagrams -->

- [x] 3.1 Add `@resvg/resvg-js` to `package.json` for in-process SVG rasterization.
- [x] 3.2 Update `skills/style/SKILL.md` to clarify when the native backend should be chosen over D2 or Excalidraw.

## 4. Verification in `extensions/render/native-diagrams/*.test.ts` <!-- specs: render/native-diagrams -->

- [x] 4.1 Add parser/validation tests for constrained motif specs.
- [x] 4.2 Add output tests for native SVG generation and panel layout behavior.
- [x] 4.3 Add PNG export plumbing tests that verify browser-free rasterization.
