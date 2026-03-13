---
name: style
description: Unified visual style guide. Defines the Alpharius color system, typography, spacing, and semantic palette shared across pi TUI theme, Excalidraw diagrams, D2 diagrams, and generated images. Use when creating any visual output to ensure consistency.
---

# Style Guide

Canonical design system for all visual output. Every diagram, theme, and generated image should derive from these tokens. When in doubt, reference this — not ad-hoc hex values.

## Design Philosophy

**Alpharius** — dark, cold, precise. Deep void backgrounds with iridescent ceramite teal, silver-chrome highlights, and muted brass-gold signal warmth. The aesthetic of the XX Legion: disciplined, mercurial, beautiful in its deception. Every surface gleams like polished power armour under starlight.

Principles:
1. **Semantic color, not decorative** — every color communicates purpose
2. **Contrast over subtlety** — void backgrounds demand bright, readable foregrounds
3. **Consistency across mediums** — same palette whether it's a TUI, an Excalidraw diagram, or a D2 chart
4. **Hierarchy through scale and weight** — not through color proliferation
5. **Document survivability matters** — technical diagrams are often embedded in documents and scaled down, so composition must preserve text and relationship legibility at page-fit sizes

---

## Color System

### Core Palette

Derived from `themes/alpharius.json`. These are the ground-truth tokens.

| Token | Hex | Role |
|-------|-----|------|
| `primary` | `#2ab4c8` | Brand accent, interactive elements, focus — iridescent ceramite |
| `primaryMuted` | `#1a8898` | Secondary accent, labels, links |
| `primaryBright` | `#6ecad8` | Headings, highlighted text — silver-teal shimmer |
| `fg` | `#c4d8e4` | Primary text — cool silver-white |
| `mutedFg` | `#607888` | Secondary text, tool output, muted content |
| `dimFg` | `#344858` | Tertiary text, comments, inactive elements |
| `bg` | `#06080e` | Main background — the void |
| `cardBg` | `#0e1622` | Elevated surface (cards, panels) |
| `surfaceBg` | `#131e2e` | Secondary surface |
| `borderColor` | `#1a3448` | Standard borders |
| `borderDim` | `#0e1e30` | Subtle borders, separators |

### Signal Colors

| Signal | Hex | Usage |
|--------|-----|-------|
| `green` | `#1ab878` | Success, completion, positive — hydra emerald |
| `red` | `#c83030` | Error, destructive, critical — blood of the false emperor |
| `orange` | `#c86418` | Warning, attention needed — hot metal |
| `yellow` | `#b89020` | Caution, numbers, highlights — tarnished brass |

### Excalidraw Semantic Palette

For diagram elements. Maps purpose → fill/stroke pairs. Defined in `extensions/render/excalidraw/types.ts`.

| Purpose | Fill | Stroke | When to Use |
|---------|------|--------|-------------|
| `primary` | `#1a4a6e` | `#2ab4c8` | Default components, neutral nodes |
| `secondary` | `#1a3a5a` | `#1a8898` | Supporting/related components |
| `tertiary` | `#0e2a40` | `#344858` | Third-level, background detail |
| `start` | `#0e2e20` | `#1ab878` | Entry points, triggers, inputs |
| `end` | `#2e2010` | `#b89020` | Outputs, completion, results |
| `decision` | `#2a1010` | `#c83030` | Conditionals, branches, choices |
| `ai` | `#1a1040` | `#6060c0` | AI/LLM components, inference |
| `warning` | `#2a1808` | `#c86418` | Warnings, degraded states |
| `error` | `#2e0e0e` | `#c83030` | Error states, failures |
| `evidence` | `#06080e` | `#1a3448` | Code snippets, data samples, dark blocks |
| `inactive` | `#0e1622` | `#344858` | Disabled, inactive, future-state |

**Text on all semantic fills:** use `#c4d8e4` (Alpharius silver-white foreground) — all fills are dark enough to support it.

### D2 Diagram Styling

When using `render_diagram` (D2), apply Alpharius colors via `style` blocks.

**Document-fit guidance for technical capability diagrams:**
- Assume the diagram may be embedded in a document and scaled down to fit a page column or page width.
- Prefer balanced aspect ratios that survive downscaling; avoid long, thin banners and tall skinny towers unless the document format explicitly demands them.
- Preserve legibility of both node text and relationship labels/arrows after reduction.
- If a layout becomes hard to read when scaled down, restructure it into a more compact cluster, multi-row arrangement, or grouped composition instead of relying on zoom.
- For capability maps, favor compact grouping and short labeled edges over sprawling left-to-right chains.

```d2
component: API Server {
  style: {
    fill: "#1a4a6e"
    stroke: "#2ab4c8"
    font-color: "#c4d8e4"
    border-radius: 8
  }
}
```

**Defaults:** D2 renders with `--theme 200` (dark) and `--layout elk`. Use semantic colors from the Excalidraw palette table above — they work identically in D2 style blocks.

**D2 connection styling:**
```d2
a -> b: label {
  style: {
    stroke: "#2ab4c8"
    font-color: "#c4d8e4"
  }
}
```

**D2 container styling (for groups/subgraphs):**
```d2
group: Infrastructure {
  style: {
    fill: "#06080e"
    stroke: "#1a3448"
    font-color: "#6ecad8"
  }

  db: Database
  cache: Redis
}
```

---

## Kitty Compatibility

Pi renders everything in 24-bit RGB — Kitty's ANSI palette does **not** affect pi's own output. `themes/alpharius.conf` is a *compatibility layer*, not a full mirror. It sets only what matters:

| What | Why it must match |
|------|-------------------|
| `background` / `foreground` | Seamless blend between pi regions and naked terminal |
| `cursor` | On-palette feel in the editor |
| `selection_background` | Readable text selection |
| Tab bar / borders | Kitty chrome the agent can't control |
| ANSI signal colours (red/green/yellow) | Shell prompt, `ls`, `git diff` use these; matching alpharius values avoids jarring contrast |

Everything else (the remaining ANSI slots) uses reasonable dark-theme defaults that won't clash. They do not need to be exact alpharius values.

### Install

```conf
# in kitty.conf:
include /path/to/omegon/themes/alpharius.conf
```

Regenerate after changing `background`, `foreground`, `cursor`, or border vars:

```bash
npx tsx scripts/export-kitty-theme.ts
```

---

## Typography

### Font Stack

| Context | Font | Family ID | Notes |
|---------|------|-----------|-------|
| Diagrams (Excalidraw) | Cascadia | `3` | Monospace, clean, technical |
| Code blocks | Cascadia | — | Matches diagram text |
| TUI | Terminal default | — | Inherits from terminal emulator |

### Scale

| Level | Size | Color | Use |
|-------|------|-------|-----|
| Title | 28px | `#2ab4c8` | Diagram titles, section headers |
| Subtitle | 20px | `#1a8898` | Sub-sections, group labels |
| Body | 16px | `#607888` | Default text, labels |
| Small | 12px | `#344858` | Annotations, fine print |

### Text on Backgrounds

| Background | Text Color | Example |
|------------|------------|---------|
| Dark (all Alpharius fills) | `#c4d8e4` | Silver-white on void |
| Transparent / no fill | Stroke color or `#607888` | Inherits from context |

---

## Spacing & Layout

### Grid

- Base unit: **20px**
- Excalidraw grid: 20px, step 5
- Minimum element gap: 20px
- Comfortable gap: 40px
- Section gap: 80px

### Element Sizes (Excalidraw)

| Scale | Width × Height | Use |
|-------|---------------|-----|
| Hero | 300 × 150 | Visual anchor, primary focus |
| Primary | 180 × 90 | Standard components |
| Secondary | 120 × 60 | Supporting elements |
| Small | 60 × 40 | Indicators, dots, badges |
| Dot | 12 × 12 | Timeline markers, bullets |

### Stroke

| Style | Width | Use |
|-------|-------|-----|
| Standard | 2px | Default for all elements |
| Emphasized | 3px | Highlighted paths, primary flow |
| Subtle | 1px | Background connections, annotations |

---

## Rendering Defaults

### D2

- `--theme 200` — dark theme
- `--layout elk` — ELK layered algorithm (cleaner than dagre for most diagrams)
- `--pad 40` — comfortable padding
- Apply Alpharius colors via style blocks (see D2 Diagram Styling above)
- D2 is the default tool for straightforward structural diagrams with regular graph layout
- When the target is a document, prefer compact page-friendly compositions over panoramic or skyscraper aspect ratios
- Treat readability at reduced size as a first-class constraint for node labels and edge relationships
- Prefer the native SVG backend for document-bound technical diagrams that fit canonical motifs such as pipeline, fanout, or panel-split and need deterministic SVG/PNG output
- Switch to Excalidraw when the diagram needs explicit whitespace lanes, trust-boundary placement, phased panels, control-plane vs data-plane separation, or other layout-sensitive spatial composition beyond the native motif set

### Excalidraw

- `roughness: 0` — clean, not hand-drawn
- `fillStyle: "solid"` — no hatching
- `strokeStyle: "solid"` — default; use `"dashed"` for optional/future
- `roundness: { type: 3 }` — adaptive corners on rectangles
- `fontFamily: 3` — Cascadia (monospace)
- `viewBackgroundColor: "#06080e"` — void black canvas
- Prefer Excalidraw for layout-sensitive capability maps, trust boundaries, phased flows, and other diagrams where local spacing and connector routing matter more than pure graph regularity
- When generating Excalidraw programmatically, keep it minimal and hand-crafted; prefer the native SVG backend for repeatable canonical document diagrams
- Use canonical layouts (`pipeline`, `fanout`, `converge`, `grid`) as the starting grammar for repeated architecture diagrams

### FLUX.1 (Image Generation)

- Use `diagram` preset (1024×768) for technical visuals
- Use `schnell` for iteration, `dev` for finals
- Quantize to `4` bits on 16GB machines
- Prompts should reference the palette by description: "iridescent blue-green ceramite on deep void-black background, silver chrome highlights, alpha legion aesthetic"

---

## Quick Reference Card

```
BACKGROUNDS          ACCENTS              SIGNALS
bg:       #06080e    primary:    #2ab4c8  green:  #1ab878
cardBg:   #0e1622    primaryMu:  #1a8898  red:    #c83030
surfaceBg:#131e2e    primaryBr:  #6ecad8  orange: #c86418
                                          yellow: #b89020

TEXT                 BORDERS
fg:       #c4d8e4    border:     #1a3448
mutedFg:  #607888    borderDim:  #0e1e30
dimFg:    #344858

EXCALIDRAW SEMANTICS (fill / stroke)
primary:   #1a4a6e / #2ab4c8    start:     #0e2e20 / #1ab878
secondary: #1a3a5a / #1a8898    end:       #2e2010 / #b89020
decision:  #2a1010 / #c83030    ai:        #1a1040 / #6060c0
warning:   #2a1808 / #c86418    error:     #2e0e0e / #c83030
evidence:  #06080e / #1a3448    inactive:  #0e1622 / #344858
```
