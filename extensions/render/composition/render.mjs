#!/usr/bin/env node
/**
 * render.mjs — CLI entry point for the composition render pipeline.
 * Browser-free React → still/GIF/MP4 using Satori + resvg + gifenc.
 *
 * Usage:
 *   node render.mjs --composition <path> --mode still|gif|mp4
 *     [--frame <n>] [--width <px>] [--height <px>] [--fps <n>]
 *     [--duration <frames>] [--output <path>] [--props <json>]
 *
 * All output is written to stdout as JSON: {ok:true,...} or {ok:false,error}.
 */

import { readFileSync, writeFileSync, mkdirSync, readdirSync } from 'node:fs';
import { resolve, dirname, extname, basename } from 'node:path';
import { tmpdir } from 'node:os';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

// ---------------------------------------------------------------------------
// Arg parsing
// ---------------------------------------------------------------------------

function parseArgs(argv) {
  const args = {};
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg.startsWith('--')) {
      const key = arg.slice(2);
      const val = argv[i + 1] && !argv[i + 1].startsWith('--') ? argv[++i] : 'true';
      args[key] = val;
    }
  }
  return args;
}

const args = parseArgs(process.argv.slice(2));

const compositionPath = args.composition;
const mode           = (args.mode || 'still').toLowerCase();
const startFrame     = parseInt(args.frame    ?? '0',   10);
const width          = parseInt(args.width    ?? '1920', 10);
const height         = parseInt(args.height   ?? '1080', 10);
const fps            = parseInt(args.fps      ?? '30',  10);
const duration       = parseInt(args.duration ?? '1',   10); // frames
const outputPath     = args.output ?? './out.png';
let   userProps      = {};

try {
  if (args.props) userProps = JSON.parse(args.props);
} catch {
  fail('--props must be valid JSON');
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function ok(extra) {
  console.log(JSON.stringify({ ok: true, ...extra }));
  process.exit(0);
}

function fail(error) {
  console.log(JSON.stringify({ ok: false, error: String(error) }));
  process.exit(1);
}

if (!compositionPath) fail('--composition is required');

// ---------------------------------------------------------------------------
// Font loading
// ---------------------------------------------------------------------------

const __filename = fileURLToPath(import.meta.url);
const __dirname  = dirname(__filename);
const fontsDir   = resolve(__dirname, 'fonts');

function loadFonts() {
  const fonts = [];
  let entries;
  try {
    entries = readdirSync(fontsDir);
  } catch {
    return fonts; // no fonts dir — satori will use system defaults
  }

  for (const file of entries) {
    if (extname(file).toLowerCase() !== '.ttf') continue;
    const namePart = basename(file, '.ttf'); // e.g. "Inter-Bold"
    const [family, weightStr] = namePart.split('-');
    const weightMap = {
      thin:        100, extralight: 200, light: 300,
      regular:     400, medium:     500, semibold: 600,
      bold:        700, extrabold:  800, black:    900,
    };
    const weight = weightMap[(weightStr ?? 'regular').toLowerCase()] ?? 400;
    const style  = namePart.toLowerCase().includes('italic') ? 'italic' : 'normal';
    try {
      const data = readFileSync(resolve(fontsDir, file));
      fonts.push({ name: family, data: data.buffer, weight, style });
    } catch {
      // skip unreadable fonts
    }
  }
  return fonts;
}

// ---------------------------------------------------------------------------
// Import user composition via jiti (handles .tsx / .ts / .js)
// ---------------------------------------------------------------------------

async function loadComposition(path) {
  const { createJiti } = await import('jiti');
  const jiti = createJiti(import.meta.url);
  const absPath = resolve(process.cwd(), path);
  const mod = await jiti.import(absPath);
  // Support both default export and named Component export
  const Component = mod?.default ?? mod?.Component;
  if (typeof Component !== 'function') {
    throw new Error(`Composition at "${path}" must have a default export that is a React component function`);
  }
  return Component;
}

// ---------------------------------------------------------------------------
// Render a single frame → raw RGBA Uint8Array
// ---------------------------------------------------------------------------

async function renderFrame(Component, frameIndex, fonts, React, satori, Resvg) {
  const frameProps = {
    frame: frameIndex,
    fps,
    durationInFrames: duration,
    width,
    height,
    props: userProps,
  };

  // Build the React element (no JSX — render.mjs is plain .mjs)
  const element = React.createElement(Component, frameProps);

  const svg = await satori(element, { width, height, fonts });

  const resvg = new Resvg(svg, {
    fitTo: { mode: 'width', value: width },
  });
  const rendered = resvg.render();
  return rendered; // ResvgRenderImage with .asPng() and .pixels (RGBA Uint8Array)
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

async function main() {
  const fonts     = loadFonts();
  const Component = await loadComposition(compositionPath);

  // Lazy-import heavy deps after validation
  const React           = (await import('react')).default;
  const { default: satori } = await import('satori');
  const { Resvg }       = await import('@resvg/resvg-js');

  // -------------------------------------------------------------------------
  // STILL mode
  // -------------------------------------------------------------------------
  if (mode === 'still') {
    const rendered = await renderFrame(Component, startFrame, fonts, React, satori, Resvg);
    const png      = rendered.asPng();
    const outPath  = resolve(process.cwd(), outputPath);
    writeFileSync(outPath, png);
    ok({ path: outPath, sizeBytes: png.byteLength });
  }

  // -------------------------------------------------------------------------
  // GIF mode
  // -------------------------------------------------------------------------
  else if (mode === 'gif') {
    const { GIFEncoder, quantize, applyPalette } = await import('gifenc');

    const gif      = GIFEncoder();
    const delayMs  = Math.round(1000 / fps);

    for (let f = 0; f < duration; f++) {
      const rendered = await renderFrame(Component, f, fonts, React, satori, Resvg);
      // pixels is a Uint8Array of RGBA bytes, width×height×4
      const pixels = new Uint8Array(rendered.pixels);
      const palette = quantize(pixels, 256, { format: 'rgba4444' });
      const index   = applyPalette(pixels, palette, 'rgba4444');
      gif.writeFrame(index, width, height, { palette, delay: delayMs });
    }

    gif.finish();
    const data    = gif.bytesView();
    const outPath = resolve(process.cwd(), outputPath);
    writeFileSync(outPath, data);
    ok({ path: outPath, sizeBytes: data.byteLength });
  }

  // -------------------------------------------------------------------------
  // MP4 mode
  // -------------------------------------------------------------------------
  else if (mode === 'mp4') {
    const tmp      = resolve(tmpdir(), `pi-render-${Date.now()}`);
    mkdirSync(tmp, { recursive: true });

    const padLen = String(duration - 1).length;

    for (let f = 0; f < duration; f++) {
      const rendered  = await renderFrame(Component, f, fonts, React, satori, Resvg);
      const png       = rendered.asPng();
      const frameName = String(f).padStart(padLen, '0') + '.png';
      writeFileSync(resolve(tmp, frameName), png);
    }

    const outPath = resolve(process.cwd(), outputPath);
    const result  = spawnSync('ffmpeg', [
      '-y',
      '-framerate', String(fps),
      '-i', resolve(tmp, `%0${padLen}d.png`),
      '-c:v', 'libx264',
      '-pix_fmt', 'yuv420p',
      '-movflags', '+faststart',
      outPath,
    ], { stdio: 'pipe' });

    if (result.status !== 0) {
      const stderr = result.stderr?.toString() ?? 'unknown ffmpeg error';
      fail(`ffmpeg failed: ${stderr}`);
    }

    ok({ path: outPath, sizeBytes: readFileSync(outPath).byteLength });
  }

  // -------------------------------------------------------------------------
  // Unknown mode
  // -------------------------------------------------------------------------
  else {
    fail(`Unknown --mode "${mode}". Valid values: still, gif, mp4`);
  }
}

main().catch((err) => fail(err?.message ?? String(err)));
