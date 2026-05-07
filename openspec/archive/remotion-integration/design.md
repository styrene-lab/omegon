+++
id = "08796283-b021-4cbf-be02-3783c2d81b77"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Remotion native support via render extension — Design

## Architecture Decisions

### Decision: Reuse Playwright Chromium via browserExecutable

**Status:** decided
**Rationale:** renderStill() and renderMedia() both accept browserExecutable: string | null. We pass the Playwright Chromium path (resolved via `playwright chromium executable-path` or the Python API). No second Chromium download needed.

### Decision: Raw .tsx for MVP, spec layer deferred

**Status:** decided
**Rationale:** Agent writes a full Remotion composition (.tsx) and passes the file path to the tool. Tool handles bundle() → renderStill()/renderMedia(). Higher-level spec abstraction deferred until MVP reveals what patterns emerge.

### Decision: Dedicated remotion/ subdirectory with own package.json

**Status:** decided
**Rationale:** @remotion/bundler pulls in webpack — too heavy to add to pi-kit's root package.json. A self-contained extensions/render/remotion/ directory with its own package.json (like excalidraw-renderer) keeps the dependency isolated. The render extension spawns a child process into that dir.

### Decision: Satori + resvg + FFmpeg instead of Chromium + Remotion renderer

**Status:** decided
**Rationale:** Remotion's renderer is hard-coupled to Chromium/puppeteer-core — no way to swap the engine. Instead: Satori renders React JSX → SVG (pure Node, no browser), resvg-js (already in render extension) rasterizes SVG → PNG per frame, FFmpeg stitches frames → MP4. No browser dependency, no Google, significantly faster per frame (~5ms vs ~100ms+). CSS subset and no-hooks constraint are acceptable for data viz / motion graphics use cases.

### Decision: Props-based frame injection instead of Remotion hooks

**Status:** decided
**Rationale:** Satori only supports pure, stateless React components — no useState, useEffect, or custom hooks. Compositions receive { frame, fps, durationInFrames, width, height } as props directly. This is actually cleaner for agent-written code: no import from 'remotion' required, easier to reason about, fully serializable.

### Decision: Bundle Tomorrow (mono) + Inter (sans) as default fonts

**Status:** decided
**Rationale:** Satori requires explicit font data — no system font fallback. Bundle Tomorrow (geometric monospace, already preferred in pi-kit style) and Inter (industry-standard sans-serif) directly in extensions/render/composition/fonts/. Both are open source. Compositions get both available by default; agent can reference font-family: 'Tomorrow' or 'Inter' without any setup.

### Decision: Dual video output: GIF (pure Node, always available) + MP4 via FFmpeg (optional)

**Status:** decided
**Rationale:** render_composition_video produces GIF by default (gifenc, pure Node, no external deps) — works everywhere, great for sharing. If FFmpeg is on PATH, also produces MP4. Tool returns both paths when available. GIF is the primary output; MP4 is the quality upgrade.

### Decision: Still output: inline PNG up to 1MB, path reference above threshold

**Status:** decided
**Rationale:** render_composition_still always writes to ~/.pi/visuals/ and returns the file path. Additionally returns inline base64 PNG if the file is ≤1MB — consistent with other render tools for normal sizes, avoids bloating context for large frames. Tool response always includes the path regardless.

### Decision: CSS subset: document in tool description, surface Satori errors verbatim

**Status:** decided
**Rationale:** No lint step. Tool description lists what's supported (Flexbox, no Grid, no box-shadow, no CSS animations). Satori errors are returned verbatim so the agent can self-correct. Adding a validator pre-render is over-engineering for MVP.

### Decision: gifenc for GIF encoding

**Status:** decided
**Rationale:** mattdesl/gifenc: fast, pure JS, supports quantize() + applyPalette() for per-frame palette reduction, worker-capable for parallelism later. Best fit for Satori output which uses limited flat-color palettes. npm package gifenc@1.0.3.

## Research Context

### Existing render extension capabilities

- **Playwright + headless Chromium** already installed for Excalidraw rendering (uv project at extensions/render/excalidraw-renderer)
- **resvg-js** for in-process PNG rasterization (native-diagrams path)
- **D2 CLI** for structural diagrams
- **mflux/FLUX.1** for AI image generation
- Output pipeline: all tools write to ~/.pi/visuals/ and return inline images
- The Playwright setup is already a uv project — adding npm deps alongside it is straightforward

### Remotion rendering modes

Remotion has two rendering paths relevant here:

1. **`@remotion/renderer` (Node.js API)** — `renderMedia()`, `renderStill()`, `renderFrames()` — programmatic rendering without the CLI. Spins up Chromium internally via `puppeteer-core`. This is the interesting path: no separate server needed, pure Node API.

2. **Remotion CLI** — `npx remotion render` — requires a project bundle. Heavier, but simpler for one-shot renders.

3. **`renderStill()`** — renders a single frame to PNG. Essentially free — same as what Excalidraw renderer does but for arbitrary React components.

The `@remotion/renderer` Node API is the right integration point: the agent writes a Remotion composition (React + TypeScript), the extension calls `renderMedia()` or `renderStill()` directly, returns the result inline. No dev server, no CLI overhead.

### Integration options

**Option A — renderStill only (image output)**
Agent writes a React component, extension bundles it with esbuild, calls `renderStill()` → PNG returned inline. Fits perfectly in the existing `~/.pi/visuals/` pipeline. No FFmpeg needed. Very fast.

**Option B — renderMedia (video output)**
Full MP4/WebM output via `renderMedia()`. Requires FFmpeg (already commonly present). Slower but produces actual video. Agent writes composition, extension renders, returns path.

**Option C — shared Chromium with Excalidraw renderer**
Reuse the existing Playwright Chromium installation rather than having Remotion download its own. `@remotion/renderer` accepts a `puppeteerInstance` or `chromiumOptions.executablePath`. Could point it at the same binary Playwright uses.

**Option D — Remotion Studio in a tool**
Spin up the Remotion dev server, open a browser preview. Heavier, more interactive. Probably out of scope for a tool.

**Recommended path: A + C**
- `render_remotion_still` tool for single-frame React → PNG (fits existing inline image pattern)
- `render_remotion_video` tool for full composition → MP4 path
- Both reuse Playwright's Chromium via `executablePath`
- Agent writes `.tsx` composition files, extension handles bundle + render lifecycle

## File Changes

- `extensions/render/remotion/package.json` (new) — Isolated npm project with @remotion/renderer, @remotion/bundler, remotion, react, react-dom deps
- `extensions/render/remotion/render.mjs` (new) — CLI entry point: reads args (composition-path, composition-id, mode, output, frame, width, height, fps, duration), calls bundle() then renderStill() or renderMedia(), prints result JSON
- `extensions/render/index.ts` (modified) — Add render_remotion_still and render_remotion_video tools; resolve Playwright Chromium path; spawn render.mjs child process
- `extensions/render/composition/package.json` (new) — Isolated npm project: satori, react, react-dom, @resvg/resvg-js (already in render/native-diagrams — may share)
- `extensions/render/composition/render.mjs` (new) — CLI entry: imports satori + resvg, dynamically imports user composition .tsx via jiti, renders frame range, writes PNG sequence to tmpdir, optionally calls ffmpeg to stitch → MP4, prints result JSON
- `extensions/render/composition/tsconfig.json` (new) — TSConfig for compositions: jsx react, moduleResolution bundler, allows agent-written .tsx to be jiti-imported
- `extensions/render/index.ts` (modified) — Add render_composition_still (single frame → PNG inline) and render_composition_video (frame sequence → MP4 path) tools
- `extensions/render/composition/package.json` (new) — satori, react, react-dom, jiti, gifenc, @resvg/resvg-js. No webpack, no Chromium.
- `extensions/render/composition/fonts/Tomorrow-Regular.ttf` (new) — Bundled Tomorrow font (geometric mono)
- `extensions/render/composition/fonts/Tomorrow-Bold.ttf` (new) — Bundled Tomorrow Bold
- `extensions/render/composition/fonts/Inter-Regular.ttf` (new) — Bundled Inter Regular (sans-serif default)
- `extensions/render/composition/fonts/Inter-Bold.ttf` (new) — Bundled Inter Bold
- `extensions/render/composition/types.ts` (new) — FrameProps interface: { frame, fps, durationInFrames, width, height, props? }. Agent imports this for type safety.
- `extensions/render/composition/render.mjs` (new) — CLI: jiti-imports .tsx default export, loops frames, calls satori() per frame, resvg PNG, writes sequence, encodes GIF via gifenc, optionally calls ffmpeg for MP4, prints JSON result
- `extensions/render/index.ts` (modified) — Add render_composition_still and render_composition_video tools. Spawn render.mjs as child process, parse result JSON, return inline PNG or paths.

## Constraints

- browserExecutable must point to the Playwright Chromium binary — resolved via `cd excalidraw-renderer && uv run python -c 'from playwright.sync_api import sync_playwright; ...'` or cached after first resolution
- @remotion/bundler uses webpack internally — bundle step may take 10-30s on first run (cached after that)
- render_remotion_video requires ffmpeg on PATH
- Composition file must use registerRoot() / <Composition> — standard Remotion entry point convention
- serveUrl returned by bundle() is a file:// URL to a temp dir — pass directly to renderStill/renderMedia
- Satori components must be pure and stateless — no hooks, no useEffect, no dangerouslySetInnerHTML
- Composition default export receives FrameProps: { frame: number, fps: number, durationInFrames: number, width: number, height: number, props?: Record<string, unknown> }
- Satori CSS: Flexbox layout only, no Grid, no position:absolute in all contexts, no box-shadow, no CSS animations (frame drives animation instead)
- Font files must be passed explicitly to satori() — no system font fallback
- resvg-js is already a dep in native-diagrams — may be importable from there or needs to be re-declared in composition/package.json
- FFmpeg required on PATH for video output — render_composition_still works without it
- jiti used for dynamic .tsx import without a build step — must be in composition/package.json deps
- FrameProps is the only import agents need — no remotion, no react-dom, just the props type
- gifenc encodes paletted GIF — 256 color limit means compositions should use limited palettes for best results
- jiti handles .tsx import without a build step but requires react/react-dom in composition/node_modules
- Satori width/height must be numbers, not strings — tool validates before invoking
- Font files loaded once at render.mjs startup and passed to every satori() call
- Frame loop is synchronous per frame — no parallelism in MVP, parallelism is a v2 concern
- Tomorrow font download: fonts.google.com/specimen/Tomorrow (OFL license). Inter: rsms.me/inter (OFL).
