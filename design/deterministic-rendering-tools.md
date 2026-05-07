+++
id = "7c67e04d-7170-453b-bd54-c2e785c1ccbf"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Deterministic rendering tools — diagrams, documents, and visualizations without AI

## Problem

The agent can write D2/Mermaid source to a file but can't render it. Users who want flowcharts, architecture diagrams, or PDF reports get text files, not visuals. The only image generation is Scry (AI diffusion models), which is the wrong tool for "render this flowchart."

These are complementary capabilities, not competing ones:
- **Scry** — creative, AI-generated images ("draw a logo," "illustrate this concept")
- **Diagram rendering** — deterministic, structured spec → visual output ("render this flowchart")
- **Document rendering** — markdown/HTML → PDF/PNG ("export this evaluation as a PDF")

## Existing infrastructure

| Component | Status | Location |
|-----------|--------|----------|
| `image` crate (codecs) | Available | Cargo.toml |
| Base64 encoding | Exported | `tools/view.rs` |
| CLI detection (`has_cmd`) | Pattern | `tools/view.rs`, `tools/whoami.rs` |
| SVG→PNG via `rsvg-convert` | Subprocess pattern | `tools/view.rs` |
| Terminal image display | Working | `tui/image.rs`, ratatui-image |
| ContentBlock::Image | Working | Tool result data URIs |
| D2 source files | 4 files | `site/diagrams/*.d2` |

## Design

### Tool 1: `render_diagram`

Renders structured diagram source to PNG/SVG. Supports multiple backends via CLI subprocess calls — same pattern as `pdftotext` in the view tool.

**Backends (detected at session start via `has_cmd`):**

| Backend | CLI | Install | Formats |
|---------|-----|---------|---------|
| D2 | `d2` | `brew install d2` / `go install oss.terrastruct.com/d2@latest` | `.d2` → SVG/PNG |
| Mermaid | `mmdc` | `npm install -g @mermaid-js/mermaid-cli` | `.mmd` → SVG/PNG |
| GraphViz | `dot` | `brew install graphviz` | `.dot` → SVG/PNG |
| PlantUML | `plantuml` | `brew install plantuml` | `.puml` → SVG/PNG |

**Tool schema:**
```json
{
  "name": "render_diagram",
  "description": "Render a diagram from source (D2, Mermaid, GraphViz, PlantUML) to an image file. Requires the corresponding CLI tool to be installed.",
  "parameters": {
    "source": "string — diagram source code",
    "format": "string — d2, mermaid, graphviz, plantuml (auto-detected from source if omitted)",
    "output_path": "string — where to write the rendered image (default: temp file)",
    "output_format": "string — png or svg (default: png)"
  }
}
```

**Implementation:**
1. Write source to a temp file with the correct extension
2. Call the CLI tool: `d2 input.d2 output.png`
3. Read the output file, encode as base64 data URI
4. Return `ContentBlock::Image` + `ContentBlock::Text` with the output path
5. If the CLI tool isn't installed, return an error with install instructions

**Auto-detection:** At session start (or on first call), probe for available backends. The tool description dynamically lists which formats are available:
```
"Render a diagram. Available backends: d2, mermaid. 
 Not installed: graphviz (brew install graphviz), plantuml (brew install plantuml)."
```

### Tool 2: `render_document`

Renders markdown or HTML to PDF. For the RFI evaluation user — write the evaluation as markdown, render to PDF, write to Obsidian vault.

**Backends:**

| Backend | CLI | Strengths |
|---------|-----|-----------|
| pandoc | `pandoc` | Markdown → PDF (via LaTeX or wkhtmltopdf) |
| weasyprint | `weasyprint` | HTML/CSS → PDF (no LaTeX, good for styled output) |
| wkhtmltopdf | `wkhtmltopdf` | HTML → PDF (simple, widely available) |

**Tool schema:**
```json
{
  "name": "render_document",
  "description": "Render markdown or HTML to PDF. Requires pandoc or weasyprint.",
  "parameters": {
    "source": "string — markdown or HTML content",
    "format": "string — markdown or html (auto-detected)",
    "output_path": "string — where to write the PDF",
    "title": "string — document title (optional, for PDF metadata)",
    "style": "string — default, academic, minimal (optional CSS preset)"
  }
}
```

### Tool 3: `render_chart`

Renders data as charts — bar, line, pie, scatter. For users who want quick data visualizations without setting up a full plotting library.

**Backend:** SVG generation via Rust (no external dependency). The `image` crate handles rasterization to PNG. Simple chart types only — not a ggplot replacement.

**Tool schema:**
```json
{
  "name": "render_chart",
  "description": "Render a simple chart (bar, line, pie, scatter) from data. No external dependencies.",
  "parameters": {
    "data": "array — data points as [{label, value}] or [[x, y]]",
    "chart_type": "string — bar, line, pie, scatter",
    "title": "string — chart title",
    "output_path": "string — where to write the image",
    "output_format": "string — png or svg (default: png)"
  }
}
```

This is the only tool that requires no external CLI — it generates SVG directly in Rust, then optionally rasterizes via the `image` crate or `resvg`.

## Implementation approach

**Phase 1: `render_diagram` with D2 + Mermaid backends**

Highest demand. D2 and Mermaid cover 90% of software architecture diagrams. Implementation is ~150 lines — write temp file, call CLI, read output, encode as data URI.

Add `resvg` + `usvg` to Cargo.toml for SVG→PNG rasterization without requiring `rsvg-convert`. These are pure Rust, no system dependencies.

**Phase 2: `render_document` with pandoc backend**

The RFI evaluation user needs this. Pandoc is the standard tool and is already detected by the view tool for document conversion.

**Phase 3: `render_chart` with native SVG generation**

Lowest urgency. Nice to have for data analysis workflows but the agent can already write data to CSV and suggest the user open it in a spreadsheet.

## Where to put it

**Core tool, not extension.** Reasons:
- No process isolation needed — it's subprocess calls to CLI tools
- Should be available in slim mode (a junior user writing a report wants PDF export)
- The auto-detection pattern already exists in view.rs
- Extensions add latency (JSON-RPC round-trip) for what's a simple subprocess call

Add `render_diagram` and `render_document` alongside the existing tools in `tools/mod.rs`, with dedicated source files `tools/render_diagram.rs` and `tools/render_document.rs`.
