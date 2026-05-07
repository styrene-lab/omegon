+++
id = "b0e2f112-0055-440d-b761-85678b8094ee"
kind = "document"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
design_docs = ["design/native-diagram-backend-mvp.md"]
last_updated = "2026-03-10"
openspec_baselines = ["render/native-diagrams.md"]
subsystem = "render"
+++

# Render

> On-device diagram and image generation — D2 diagrams, FLUX.1 images via MLX, Excalidraw rendering, and native SVG diagramming.

## What It Does

The render extension provides multiple image generation backends:

- **D2 diagrams** (`render_diagram` tool): Renders D2 source to inline PNG via the d2 CLI. Uses elk layout engine, dark theme (200), with Alpharius semantic colors.
- **FLUX.1 images** (`generate_image_local` tool): Text-to-image generation on Apple Silicon via MLX. Presets: schnell (fast), dev (quality), diagram, portrait, wide. Quantization support (3-8 bit).
- **Excalidraw** (`render_excalidraw` tool): Renders `.excalidraw` JSON files to PNG via Playwright + headless Chromium.
- **Native SVG diagrams**: Programmatic SVG generation for architecture diagrams, flowcharts, and visual arguments without external dependencies.

Output persisted to `~/.pi/visuals/` for reuse.

## Key Files

| File | Role |
|------|------|
| `extensions/render/index.ts` | Extension entry — `render_diagram`, `generate_image_local`, `render_excalidraw` tools |
| `extensions/render/excalidraw/index.ts` | Excalidraw rendering via Playwright |
| `extensions/render/excalidraw/elements.ts` | Excalidraw element builders |
| `extensions/render/excalidraw/types.ts` | Excalidraw element types |
| `extensions/render/native-diagrams/index.ts` | Native SVG diagram pipeline |
| `extensions/render/native-diagrams/scene.ts` | Scene graph for diagram layout |
| `extensions/render/native-diagrams/svg.ts` | SVG generation from scene graph |
| `extensions/render/native-diagrams/raster.ts` | SVG → PNG rasterization |
| `extensions/render/native-diagrams/spec.ts` | Diagram specification types |
| `extensions/render/native-diagrams/motifs.ts` | Reusable visual motifs (arrows, boxes, labels) |

## Design Decisions

- **D2 over Mermaid**: D2 produces better layouts with elk engine and supports the dark theme natively. Mermaid not supported.
- **On-device FLUX.1 via MLX**: Zero API cost image generation. Quantization to 4-8 bits reduces memory pressure on shared Apple Silicon GPU.
- **Native SVG as fallback**: When d2 isn't installed, programmatic SVG generation provides basic diagramming without external dependencies.
- **Alpharius semantic colors**: Consistent color system across D2, Excalidraw, and native diagrams. Primary (#3b82f6), start (#c2410c), end (#047857), decision (#b45309), AI (#6d28d9).

## Constraints & Known Limitations

- D2 CLI required for `render_diagram` (installed via Nix or brew)
- FLUX.1 requires Apple Silicon with sufficient memory (quantize for smaller GPUs)
- Excalidraw rendering requires Playwright + Chromium (first-time setup: `uv sync && uv run playwright install chromium`)
- Native diagrams are SVG-first — rasterization to PNG requires `rsvg-convert`

## Related Subsystems

- [View](view.md) — displays rendered diagrams inline with scale controls
