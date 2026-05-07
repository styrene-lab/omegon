+++
id = "1d61ccdd-5d16-42fc-af41-bef38117ebfd"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Remotion native support via render extension — Tasks

## 1. Composition runner (extensions/render/composition/)
<!-- Core: isolated npm project + CLI renderer -->

- [x] 1.1 Create `extensions/render/composition/package.json` with deps: `satori`, `react`, `react-dom`, `jiti`, `gifenc`, `@resvg/resvg-js`
- [x] 1.2 Download Tomorrow-Regular.ttf, Tomorrow-Bold.ttf (OFL, fonts.google.com/specimen/Tomorrow) to `extensions/render/composition/fonts/`
- [x] 1.3 Download Inter-Regular.ttf, Inter-Bold.ttf (OFL, rsms.me/inter) to `extensions/render/composition/fonts/`
- [x] 1.4 Create `extensions/render/composition/types.ts` — export `FrameProps` interface: `{ frame: number, fps: number, durationInFrames: number, width: number, height: number, props?: Record<string, unknown> }`
- [x] 1.5 Run `npm install` in `extensions/render/composition/`

## 2. render.mjs CLI entry point (extensions/render/composition/render.mjs)
<!-- Spawned by render extension; reads args, renders, prints JSON result -->

- [x] 2.1 Parse CLI args: `--composition <path>` `--mode still|gif|mp4` `--frame <n>` `--width <n>` `--height <n>` `--fps <n>` `--duration <n>` `--output <path>` `--props <json>`
- [x] 2.2 Load fonts at startup: read all `.ttf` files from `fonts/` dir, build satori font array with correct `name` and `weight` fields
- [x] 2.3 jiti-import the composition file's default export (pure stateless component)
- [x] 2.4 **Still mode**: call `satori(jsx, { width, height, fonts })` → SVG string → resvg PNG buffer → write to `--output`, print `{ ok: true, path, sizeBytes }`
- [x] 2.5 **GIF mode**: loop frames 0..durationInFrames-1, satori → resvg per frame, collect RGBA buffers; encode with gifenc (`quantize` + `applyPalette` per frame); write animated GIF to `--output`, print `{ ok: true, path, frames, sizeBytes }`
- [x] 2.6 **MP4 mode**: render PNG sequence to tmpdir (same loop as GIF), spawn ffmpeg `ffmpeg -r <fps> -i frame%04d.png -pix_fmt yuv420p <output>`, print `{ ok: true, path, frames, sizeBytes }`
- [x] 2.7 Wrap all execution in try/catch; on error print `{ ok: false, error: message }` to stdout and exit 1
- [x] 2.8 Validate: width/height must be positive integers; durationInFrames ≥ 1; fps 1–120

## 3. render extension tools (extensions/render/index.ts)
<!-- Two new tools wired into the existing extension -->

- [x] 3.1 Add `render_composition_still` tool: params `composition_path` (string), `frame` (number, default 0), `width` (number, default 1920), `height` (number, default 1080), `props` (object, optional)
- [x] 3.2 `render_composition_still` implementation: resolve abs path, spawn `node render.mjs --mode still ...` in `extensions/render/composition/`, parse JSON result; if `sizeBytes ≤ 1MB` return inline PNG attachment + path; else return path only
- [x] 3.3 Add `render_composition_video` tool: params `composition_path`, `fps` (default 30), `duration_in_frames` (number), `width` (default 1920), `height` (default 1080), `props` (optional), `format` (enum `gif|mp4|both`, default `gif`)
- [x] 3.4 `render_composition_video` implementation: output path in `~/.pi/visuals/`, spawn `node render.mjs --mode gif|mp4 ...`, if `both` run gif then mp4 sequentially; return result with paths and frame count
- [x] 3.5 Add tool descriptions documenting the Satori CSS subset (Flexbox only, no Grid, no box-shadow, no CSS animations, props-based frame injection)

## 4. Smoke test
- [x] 4.1 Write a minimal composition `extensions/render/composition/test-comp.tsx` — a progress bar animating from 0→100% over 30 frames using Verdant colors
- [x] 4.2 Run `node render.mjs --composition test-comp.tsx --mode still --frame 15 --width 800 --height 200 --output /tmp/test-still.png` — verify PNG written
- [x] 4.3 Run `node render.mjs --composition test-comp.tsx --mode gif --fps 30 --duration 30 --width 800 --height 200 --output /tmp/test.gif` — verify animated GIF written and opens correctly
- [x] 4.4 Verify `render_composition_still` tool works end-to-end from the extension (inline return for ≤1MB)
