---
id: core-distribution
title: Core binary distribution — shipping Rust alongside and eventually instead of TypeScript
status: implemented
parent: rust-agent-loop
tags: [distribution, packaging, npm, rust, ci, versioning, cross-platform]
open_questions: []
issue_type: feature
priority: 1
---

# Core binary distribution — shipping Rust alongside and eventually instead of TypeScript

## Overview

Omegon today ships as a single npm package (`npm install -g omegon`, ~191MB unpacked) containing the TypeScript runtime + vendored pi-mono. The Rust core lives in a separate repo (styrene-lab/omegon-core) submoduled at `core/`. This node defines how the Rust binary gets built, distributed, versioned, and integrated — from Phase 0 (binary bundled alongside npm package for cleave children) through Phase 3 (Rust binary IS the product, Node.js optional).

## Research

### How other projects distribute Rust binaries via npm — the proven patterns



### The esbuild/swc model (platform-specific optional dependencies)

Used by: esbuild, swc, turbo, Biome, oxlint, Prisma, tree-sitter CLI.

The pattern:
1. A meta-package (`omegon`) declares platform-specific packages as `optionalDependencies`
2. Each platform package contains exactly one prebuilt binary
3. npm's resolver installs only the package matching the current platform
4. The meta-package finds and runs the binary from the platform package

```json
{
  "name": "omegon",
  "optionalDependencies": {
    "@omegon/darwin-arm64": "0.11.0",
    "@omegon/darwin-x64": "0.11.0",
    "@omegon/linux-x64": "0.11.0",
    "@omegon/linux-arm64": "0.11.0"
  }
}
```

Each platform package is ~3-8MB (release + strip + LTO). npm only downloads the one matching the platform. The user runs `omegon` which resolves to the native binary.

**Advantages:**
- Single `npm install -g omegon` — no separate binary install step
- Automatic platform detection
- Atomic versioning — meta-package and platform packages always match
- CI/CD is standard GitHub Actions matrix with cross-compilation
- Users don't need Rust toolchain installed

**Disadvantages:**
- Need to publish 5+ packages per release (meta + platforms)
- npm OIDC trusted publishing needs to be set up per package
- The meta-package needs a resolver script that finds the right platform binary

### The GitHub Release + postinstall model

Used by: Prisma (older versions), Playwright, some Deno tools.

The pattern:
1. The npm package's `postinstall` script downloads the correct binary from GitHub Releases
2. Binary is saved to `node_modules/.cache/omegon/` or similar
3. Fallback: if download fails, try cargo install

**Advantages:**
- Only one npm package to publish
- Binaries hosted on GitHub (free, fast CDN)

**Disadvantages:**
- postinstall scripts are disabled by some security-conscious setups
- Network dependency during install
- GitHub rate limiting on anonymous downloads

### The standalone binary model

Used by: ripgrep, fd, bat, delta, Deno, Bun.

The pattern:
- GitHub Releases with platform tarballs
- Homebrew formula: `brew install omegon`
- Nix flake: `nix run github:styrene-lab/omegon-core`
- cargo install: `cargo install omegon` (builds from source)
- AUR, apt, etc.

**Advantages:**
- Clean, native install
- No npm dependency
- Small binary (~5MB vs 191MB npm package)

**Disadvantages:**
- Multiple distribution channels to maintain
- Users must install separately from the npm package (during transition)
- No atomic versioning with the TypeScript side

### What fits the Ship of Theseus transition

During the transition, both TypeScript and Rust need to be distributed *together*. The user does `npm install -g omegon` and gets both — the TS runtime for the interactive session and the Rust binary for cleave children (Phase 0) and eventually as the main entry point (Phase 1+).

The **esbuild/swc model** is the right fit:
- Single install command
- The npm package is already the distribution channel
- Platform binaries are bundled automatically
- When the Rust binary becomes the entry point (Phase 1), the npm `bin` field just points to it instead of `bin/omegon.mjs`
- When Node.js becomes optional (Phase 3), the meta-package shrinks to just the Rust binary + the LLM bridge JS

After the transition completes, the standalone binary model can be added *alongside* npm — Homebrew, Nix, GitHub Releases — for users who don't want Node.js at all. But npm remains the primary channel throughout.

### Versioning strategy — one version, two repos, converging



### The problem

Today: `omegon` npm package has version 0.10.7. The `omegon-core` Rust repo is at 0.1.0. They're different projects with different version numbers. But they're converging — eventually `omegon-core` IS `omegon`.

### The solution: omegon version is the product version, core follows

**One version number for the product.** The `omegon` npm package.json version is the authoritative version. The `omegon-core` Cargo.toml version tracks it. When omegon bumps to 0.11.0, omegon-core bumps to 0.11.0. They're the same product.

**How this works in practice:**

1. Development happens in both repos. omegon (TypeScript) and omegon-core (Rust) are developed in parallel.

2. When a release is cut, both repos are tagged with the same version:
   - `omegon` repo: `v0.11.0` → npm publish
   - `omegon-core` repo: `v0.11.0` → build platform binaries, publish platform npm packages

3. The omegon repo's submodule pin (`core/`) points to the matching omegon-core tag.

4. CI in the omegon repo triggers the omegon-core build as part of its publish workflow.

### The CI flow

```
Push to omegon main (version bumped in package.json)
  │
  ├── Build omegon-core for all platforms (matrix: darwin-arm64, darwin-x64, linux-x64, linux-arm64)
  │   ├── cargo build --release (with LTO + strip)
  │   └── Upload artifacts
  │
  ├── Publish platform packages (@omegon/darwin-arm64, etc.)
  │   └── Each contains the prebuilt binary
  │
  └── Publish omegon meta-package
      └── Contains: TypeScript runtime + optionalDependencies on platform packages
```

This is a single CI workflow in the omegon repo. It builds the Rust binary from the `core/` submodule, publishes the platform packages, then publishes the meta-package. Atomic release.

### Semantic versioning during the transition

The version communicates what changed:

- **Patch** (0.10.x): Bug fixes in either TypeScript or Rust. No new Phase 0 capabilities.
- **Minor** (0.x.0): New features, new lifecycle phases, new tools in the Rust core.
- **Major** (x.0.0): Breaking changes. Phase 1 (process inversion) is a major version bump — the binary entry point changes.

**Phase milestone versions:**
- 0.11.0: Phase 0 — Rust binary ships alongside, used for cleave children
- 0.12.0-0.x.0: Rust core gains tools, lifecycle engine, feature crates
- 1.0.0: Phase 1 — Rust binary becomes the process owner. The binary the user runs is `omegon` (Rust), not `bin/omegon.mjs` (Node.js). This is the breaking change that warrants a major bump.
- 1.x.0: Phase 2 — Native TUI replaces pi-tui bridge
- 2.0.0 (maybe): Phase 3 — Node.js becomes fully optional

### The npm scope question

The platform packages need a scope. Options:
- `@omegon/darwin-arm64` — clean, product-aligned, requires npm org "omegon"
- `@styrene-lab/omegon-darwin-arm64` — uses existing org
- `@omegon-core/darwin-arm64` — separate scope for the Rust binary

Recommendation: `@omegon/` scope. Register the `omegon` npm org. The platform packages are `@omegon/darwin-arm64`, `@omegon/darwin-x64`, `@omegon/linux-x64`, `@omegon/linux-arm64`. The meta-package stays as unscoped `omegon`.

### The CI matrix — building Rust for 4 platforms



### Target platforms

| Platform | Runner | Target triple | Notes |
|----------|--------|---------------|-------|
| macOS ARM (M1+) | `macos-latest` | `aarch64-apple-darwin` | Primary dev platform |
| macOS x86 | `macos-13` | `x86_64-apple-darwin` | Intel Macs, Rosetta |
| Linux x86_64 | `ubuntu-latest` | `x86_64-unknown-linux-gnu` | Servers, CI, WSL, k8s pods |
| Linux ARM64 | `ubuntu-24.04-arm` or cross-compile | `aarch64-unknown-linux-gnu` | ARM servers, Raspberry Pi, Graviton |

### The build matrix (GitHub Actions)

```yaml
strategy:
  matrix:
    include:
      - target: aarch64-apple-darwin
        os: macos-latest
        npm-pkg: "@omegon/darwin-arm64"
      - target: x86_64-apple-darwin
        os: macos-13
        npm-pkg: "@omegon/darwin-x64"
      - target: x86_64-unknown-linux-gnu
        os: ubuntu-latest
        npm-pkg: "@omegon/linux-x64"
      - target: aarch64-unknown-linux-gnu
        os: ubuntu-24.04-arm
        npm-pkg: "@omegon/linux-arm64"
```

Each matrix job:
1. `cargo build --release --target ${{ matrix.target }}` in the `core/` submodule
2. `strip` the binary (~50% size reduction)
3. Package into the platform npm package (just the binary + a package.json)
4. `npm publish` the platform package

### Binary size optimization

```toml
[profile.release]
lto = "fat"        # link-time optimization across all crates
strip = true       # strip debug symbols
codegen-units = 1  # maximize optimization (slower build, smaller binary)
opt-level = "z"    # optimize for size over speed
panic = "abort"    # no unwinding machinery
```

Expected size: 3-8MB per platform. Compared to the current 191MB npm package, this is a 95%+ reduction for the core binary. The full npm package will still be large during the transition (TypeScript runtime + binary), but shrinks as TypeScript components are eliminated.

### Cross-compilation alternatives

If native ARM runners aren't available:
- **cross** (https://github.com/cross-rs/cross): Docker-based cross-compilation. `cross build --target aarch64-unknown-linux-gnu`
- **zig as linker**: `cargo zigbuild` can cross-compile to any target from any host. Used by many Rust projects for CI.

### sqlite bundling

The workspace already specifies `rusqlite` with `features = ["bundled"]`. This compiles sqlite from C source into the binary — no system sqlite dependency. The binary is fully self-contained.

### How the npm package changes across phases



### Phase 0 (current → 0.11.0): Binary alongside

```json
{
  "name": "omegon",
  "version": "0.11.0",
  "bin": {
    "omegon": "bin/omegon.mjs",
    "pi": "bin/pi.mjs"
  },
  "optionalDependencies": {
    "@omegon/darwin-arm64": "0.11.0",
    "@omegon/darwin-x64": "0.11.0",
    "@omegon/linux-x64": "0.11.0",
    "@omegon/linux-arm64": "0.11.0"
  },
  "dependencies": {
    "@styrene-lab/pi-coding-agent": "...",
    "@styrene-lab/pi-ai": "...",
    "@styrene-lab/pi-tui": "..."
  }
}
```

The binary is present but only used internally — the cleave dispatcher finds `omegon-agent` from the platform package and spawns it for children instead of `pi -p --no-session`.

A resolver module (`lib/core-binary.js`) finds the binary:
```javascript
// Find the omegon-agent binary from platform packages
function findCoreBinary() {
  const platform = `${process.platform}-${process.arch}`;
  const map = { 'darwin-arm64': '@omegon/darwin-arm64', ... };
  const pkg = map[platform];
  if (!pkg) return null;
  try {
    return path.join(require.resolve(`${pkg}/package.json`), '..', 'omegon-agent');
  } catch { return null; }
}
```

### Phase 1 (1.0.0): Binary as entry point

```json
{
  "name": "omegon",
  "version": "1.0.0",
  "bin": {
    "omegon": "bin/omegon-wrapper.mjs"
  },
  "optionalDependencies": {
    "@omegon/darwin-arm64": "1.0.0",
    ...
  },
  "dependencies": {
    "@styrene-lab/pi-ai": "...",
    "bridge.mjs": "bundled"
  }
}
```

`bin/omegon-wrapper.mjs` is a 5-line script that finds and executes the Rust binary:
```javascript
#!/usr/bin/env node
import { findCoreBinary } from '../lib/core-binary.js';
import { execFileSync } from 'child_process';
const bin = findCoreBinary();
if (!bin) { console.error('No binary for this platform'); process.exit(1); }
execFileSync(bin, process.argv.slice(2), { stdio: 'inherit' });
```

The pi-* dependencies stay because the Rust binary spawns the Node.js LLM bridge subprocess, which imports pi-ai. But pi-coding-agent and pi-tui may be gone (the Rust binary owns the agent loop and TUI).

### Phase 2-3: Binary IS the package

```json
{
  "name": "omegon",
  "version": "2.0.0",
  "bin": {
    "omegon": "bin/omegon-wrapper.mjs"
  },
  "optionalDependencies": {
    "@omegon/darwin-arm64": "2.0.0",
    ...
  },
  "dependencies": {
    "@styrene-lab/pi-ai": "..."
  }
}
```

The npm package is now just: wrapper script + platform binary + pi-ai (for the LLM bridge subprocess). Total size: ~10-15MB instead of 191MB.

If the user doesn't need npm at all (Phase 3 with native Anthropic/OpenAI clients):
```bash
# Standalone install — no Node.js required
brew install omegon
# or
curl -fsSL https://omegon.dev/install.sh | sh
```

### The parallel standalone channel

From Phase 0 onward, the Rust binary is also published as:
- GitHub Releases: platform tarballs attached to each release tag
- Homebrew formula (eventually): `brew install styrene-lab/tap/omegon`

This doesn't replace npm — it's an alternative for users who want just the binary. During Phase 0-1, standalone users miss the TypeScript features. From Phase 2+, standalone is fully functional.

### Review notes — issues and refinements



### 1. Binary size estimate is optimistic once tree-sitter ships

The 3-8MB estimate assumes a pure Rust binary with sqlite bundled. But the `understand` tool depends on tree-sitter with C grammars for each supported language. Each grammar adds ~1-3MB:

| Grammar | Approx. size |
|---------|-------------|
| tree-sitter-typescript | ~2MB |
| tree-sitter-rust | ~1.5MB |
| tree-sitter-python | ~1.5MB |
| tree-sitter-go | ~1MB |
| tree-sitter-c/cpp | ~2MB |
| tree-sitter + runtime | ~0.5MB |

With 5+ grammars the binary is more likely **10-20MB**, not 3-8MB. Still a 90%+ reduction from 191MB, but the estimate in the CI research should be corrected. `opt-level = "z"` (size) helps here but has a real performance cost — tree-sitter parsing and scope graph traversal are CPU-bound work where `opt-level = 3` (speed) matters.

**Recommendation:** Use `opt-level = 3` (speed) instead of `opt-level = "z"` (size). The binary is ~2-3MB larger but the tree-sitter index builds in 2s instead of 4s for a 50k-line codebase. Binary size is less important than startup/query performance for an interactive agent loop. The npm platform packages are already per-platform (user downloads one), so the extra MB is negligible.

### 2. macOS x86_64 runner deprecation

GitHub deprecated `macos-13` (the last x86 runner) and `macos-14+` are all ARM. Cross-compilation from ARM to x86 is straightforward: `rustup target add x86_64-apple-darwin && cargo build --target x86_64-apple-darwin`. No dedicated x86 runner needed. The build matrix should use `macos-latest` for both macOS targets.

### 3. Empty core releases when only TypeScript changes

The "single version" approach means every omegon version bump also bumps omegon-core, even if the Rust code didn't change. This creates empty releases — version bumps with no code change.

**Refined approach:** Platform packages are only *published* when the core/ submodule actually changed. The meta-package always publishes. If core didn't change, the optionalDependencies point to the previous platform package version (which is still valid — npm doesn't require exact match for optionalDeps).

```yaml
- name: Check if core changed
  id: core-check
  run: |
    PREV_SHA=$(git log -2 --format='%H' -- core/ | tail -1)
    CURR_SHA=$(git log -1 --format='%H' -- core/)
    if [ "$PREV_SHA" = "$CURR_SHA" ]; then
      echo "changed=false" >> $GITHUB_OUTPUT
    else
      echo "changed=true" >> $GITHUB_OUTPUT
    fi
```

Platform packages only rebuild and publish when `core-check.outputs.changed == 'true'`. The meta-package updates its optionalDependencies version only when new platform packages are published.

### 4. No Windows target — intentional and should be explicit

The matrix has 4 targets, all Unix. This is correct — Omegon doesn't support Windows today (pi/Claude Code doesn't either). But the decision should be explicit: "Windows is not a target. WSL users are served by the linux-x64 binary."

### 5. OIDC trusted publishing per platform package

Each @omegon/ platform package needs its own npm trusted publisher configuration. That's 4 packages × 1 GitHub Actions workflow. This is a one-time setup but it's 4 separate npm org → GitHub repo connections. If we use @styrene-lab/ scope instead, the existing trusted publisher might cover all packages under that scope. Worth testing before committing to @omegon/ scope.

### 6. The Phase 1 wrapper is correct but should forward signals

`execFileSync(bin, args, { stdio: 'inherit' })` works for launching the Rust binary and inheriting terminal I/O. It also blocks the Node process (which is what we want — Node is just the launcher). Signal forwarding (SIGINT, SIGTERM) to the child works correctly with `stdio: 'inherit'` because the child is in the same process group. No changes needed.

### Installation paths — what exists now and what users need

**Current state: the Rust binary runs but has no distribution story.**

To run `omegon-agent interactive` today, a user needs:
1. The compiled Rust binary (7.5MB, arm64 macOS — must `cargo build --release` from source)
2. Node.js on PATH (for the LLM bridge subprocess)
3. `@styrene-lab/pi-ai` installed somewhere Node can find it (the bridge `import`s `streamSimple`)
4. The `llm-bridge.mjs` file at a discoverable path relative to the binary
5. API keys (ANTHROPIC_API_KEY, etc.) in environment or `~/.pi/agent/settings.json`

**Three viable installation paths, simplest first:**

**Path A: npm install (transition path — ships binary inside npm package)**
```
npm install -g omegon
```
User gets: the TS Omegon (current) + the Rust binary as a platform-specific optional dep. The `omegon` command runs the TS interactive mode (unchanged). The Rust binary is used internally for cleave children and available as `omegon-agent` for headless/interactive use. Bridge and pi-ai come with the npm package.

This is the `core-distribution` design node's approach. Requires: platform npm packages (@omegon/darwin-arm64 etc.), CI build matrix.

**Path B: Standalone binary + npm bridge package (new, minimal)**
```
# Install the binary (one of):
brew install omegon        # Homebrew
curl -fsSL https://omegon.dev/install.sh | sh  # Script
cargo install omegon       # From source

# Install the LLM bridge (one-time):
npm install -g @styrene-lab/pi-ai
```
User gets: the Rust binary + Node.js as a runtime dependency for the bridge. The binary auto-discovers pi-ai via `node -e "import(...)"`. This is the leanest path — no 191MB npm package, no TS runtime, just the binary + bridge.

**Path C: Fully standalone (Phase 3 — no Node.js)**
```
brew install omegon  # or curl | sh
```
User gets: the Rust binary with native Anthropic/OpenAI HTTP clients. No Node.js needed. This is Phase 3 — requires implementing reqwest-based provider clients.

**What's blocking Path A:** CI build matrix for 4 platforms, npm trusted publishing for platform packages.
**What's blocking Path B:** A `brew` formula or install script, and the bridge's pi-ai discovery.
**What's blocking Path C:** Native Rust HTTP clients for Anthropic + OpenAI streaming.

**The immediate question: what's the cheapest path to "someone else can install and run this"?**

Path B is cheapest. It needs:
1. GitHub Release with the binary (we already build `cargo build --release`)
2. The bridge bundled next to the binary (copy `bridge/llm-bridge.mjs` alongside)
3. A simple install script that downloads the binary + bridge for the platform
4. Documentation: "install Node.js, run `npm install -g @styrene-lab/pi-ai`, then run `omegon-agent interactive`"

Path A is more polished (single `npm install`) but requires the platform package infrastructure.

**Recommendation: Path B first (unblocks testing), then Path A for production distribution.**

### Exact npm/Node.js dependency chain — what must become Rust

**The single Node.js dependency is the LLM bridge (307 lines of JS).** It does three things:

1. **API key resolution** — reads env vars or `~/.pi/agent/auth.json` for OAuth tokens. **Trivially portable to Rust** — it's just file reads + JSON parsing. ~30 lines of Rust.

2. **Model resolution** — maps `"anthropic:claude-sonnet-4-20250514"` to a provider + model config. **Trivially portable** — it's a lookup table. ~20 lines of Rust.

3. **LLM streaming** — calls `streamSimple(model, context, options)` from `@styrene-lab/pi-ai`, which delegates to the provider-specific SDK. **This is the actual dependency.**

**What `streamSimple` does for the Anthropic provider (our primary):**
```
streamSimple(model, context, options)
  → anthropic.js: streamSimpleAnthropic()
    → new Anthropic({apiKey, baseUrl})
    → client.messages.create({model, messages, system, tools, stream: true})
    → iterate SSE stream, emit events (text_delta, tool_use, thinking, etc.)
```

The Anthropic SDK (`@anthropic-ai/sdk`) is an HTTP client that:
- POST to `https://api.anthropic.com/v1/messages` with JSON body
- Sets headers: `x-api-key`, `anthropic-version: 2023-06-01`, `content-type: application/json`
- Reads SSE (Server-Sent Events) streaming response
- Parses `event: content_block_delta` / `data: {"type":"content_block_delta","delta":{"text":"..."}}` lines

**That's it.** The entire npm dependency tree exists to make one HTTP POST with SSE streaming.

**What Rust needs to replace it:**

```rust
// 1. Build the request
let body = json!({
    "model": "claude-sonnet-4-20250514",
    "max_tokens": 8192,
    "system": system_prompt,
    "messages": messages,
    "tools": tools,
    "stream": true,
});

// 2. Send it
let response = reqwest::Client::new()
    .post("https://api.anthropic.com/v1/messages")
    .header("x-api-key", &api_key)
    .header("anthropic-version", "2023-06-01")
    .header("content-type", "application/json")
    .json(&body)
    .send()
    .await?;

// 3. Parse SSE stream
let mut stream = response.bytes_stream();
// Parse "event: " and "data: " lines, emit typed events
```

**For OpenAI (secondary):**
Same pattern, different URL (`https://api.openai.com/v1/chat/completions`), different headers (`Authorization: Bearer {key}`), different SSE format (`data: {"choices":[{"delta":{"content":"..."}}]}`).

**The dependency is ~200-400 lines of Rust per provider.** We already have `reqwest` as a dependency (for web_search). SSE parsing is ~50 lines of state machine code.

**What we DON'T need:**
- The Anthropic JS SDK (3,000+ files, ~2MB)
- The OpenAI JS SDK (similar)
- The 8 other provider SDKs (Bedrock, Google, Mistral, etc.)
- Node.js runtime
- npm

**The 95% case:** Anthropic + OpenAI. These two cover >95% of actual usage. Bedrock/Google/Mistral are nice-to-have. We can add them later or keep the bridge as an optional fallback for long-tail providers.

### Post-E2E reassessment — what changed and what decisions are stale

**The E2E test proved the binary is fully self-contained.** No Node.js, no npm, no bridge subprocess. This invalidates several decisions made when we assumed the Node.js bridge was permanent.

**Stale decisions:**
1. ~~"Standalone install: binary from GitHub Release + `npm install -g @omegon/bridge` for LLM providers"~~ → The bridge is no longer needed. Native Anthropic + OpenAI clients handle >95% of usage. Install is: download binary + set API key (or run `login`).

2. ~~"Platform-specific npm packages for the Rust binary (esbuild/swc model)"~~ → npm distribution is still valid for the **transition period** (where TS Omegon coexists with Rust), but is no longer the primary path. The primary path is standalone binary distribution.

3. ~~"CI builds Rust binary from the core/ submodule as part of the omegon publish workflow"~~ → The Rust binary should have its own CI workflow (GitHub Releases on tag push), independent of the npm publish workflow. They can be coordinated but shouldn't be coupled.

**What's current:**

| Channel | Status | When |
|---------|--------|------|
| **GitHub Releases** (primary) | Ready to implement | Now — just need CI + release workflow |
| **`brew install`** | Future | After GitHub Releases are working |
| **`curl \| sh` installer** | Future | After GitHub Releases are working |
| **npm platform packages** | Transition only | While TS Omegon is still the interactive mode |
| **cargo install** | Optional | Always available from source |

**What the CI needs:**
1. GitHub Actions workflow triggered on version tags (v0.12.0, etc.)
2. Build matrix: darwin-arm64, darwin-x64, linux-x64, linux-arm64
3. Upload tarballs as release assets
4. Each tarball contains: `omegon-agent` binary (that's it — one file)

**What the install script needs:**
```bash
#!/bin/sh
# Detect platform
ARCH=$(uname -m)
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
# Download + install
curl -fsSL "https://github.com/styrene-lab/omegon-core/releases/latest/download/omegon-agent-${OS}-${ARCH}.tar.gz" | tar xz
sudo mv omegon-agent /usr/local/bin/
echo "Installed. Run: omegon-agent login (subscription) or set ANTHROPIC_API_KEY"
```

**Binary size:** 7.7MB (arm64 macOS release). Acceptable — rustls + reqwest + ratatui + sqlite are the bulk. Could drop to ~5MB with `opt-level = "z"` + strip, but speed matters more (per existing decision).

## Decisions

### Decision: Platform-specific npm packages for the Rust binary (esbuild/swc model)

**Status:** decided
**Rationale:** The `omegon` npm meta-package declares platform-specific packages (@omegon/darwin-arm64, @omegon/darwin-x64, @omegon/linux-x64, @omegon/linux-arm64) as optionalDependencies. npm installs only the matching platform. A resolver module finds the binary at runtime. This is the proven model used by esbuild, swc, turbo, Biome, and oxlint. Single `npm install -g omegon` gives users both the TypeScript runtime and the Rust binary. No separate install step, no postinstall downloads, no Rust toolchain required. Platform packages are ~3-8MB each (release + LTO + strip).

### Decision: Single version number across omegon and omegon-core — the product version is the npm version

**Status:** decided
**Rationale:** One version for the product. omegon package.json version is authoritative. omegon-core Cargo.toml tracks it. Both repos are tagged with the same version on release. The submodule pin tracks which core version is integrated. CI builds both and publishes atomically from the omegon repo. Phase 1 (process inversion) is the 1.0.0 major version bump — the entry point changes from Node.js to Rust. Pre-1.0 versions are the Ship of Theseus transition.

### Decision: CI builds Rust binary from the core/ submodule as part of the omegon publish workflow

**Status:** decided
**Rationale:** The omegon repo's publish.yml workflow gains a matrix job that builds omegon-core for 4 platforms (darwin-arm64, darwin-x64, linux-x64, linux-arm64), packages each into a platform npm package, and publishes them before the meta-package. This is atomic — all platform packages and the meta-package share the same version and publish in the same CI run. Build optimization: release profile with LTO=fat, strip=true, codegen-units=1, opt-level=z, panic=abort. sqlite is bundled (no system dependency). Expected binary size: 3-8MB per platform.

### Decision: GitHub Releases as parallel standalone channel from Phase 0 onward

**Status:** decided
**Rationale:** Every release publishes platform tarballs as GitHub Release assets alongside the npm packages. This serves: k8s pod deployments (no Node.js needed in the image), CI environments, and users who want just the binary. During Phase 0-1, standalone misses TypeScript features. From Phase 2+, standalone is fully functional. Homebrew formula can be added when demand warrants. npm remains the primary channel throughout the transition.

### Decision: Use @omegon/ scope if registerable, fall back to @styrene-lab/omegon-* prefix

**Status:** decided
**Rationale:** The @omegon/ scope is cleaner (@omegon/darwin-arm64 vs @styrene-lab/omegon-darwin-arm64). npm org registration is free for public packages. If the omegon org name is taken or unavailable, fall back to @styrene-lab/ with an omegon- prefix. Either way the meta-package stays as unscoped `omegon`. This is a cosmetic choice that doesn't affect architecture — resolve it when setting up the first platform package publish.

### Decision: opt-level = 3 (speed) over opt-level = z (size) — tree-sitter parsing performance matters more than binary size

**Status:** decided
**Rationale:** The understand tool's tree-sitter grammars add 8-15MB to the binary. With `opt-level = "z"` (size) the binary is ~10-15MB; with `opt-level = 3` (speed) it's ~13-18MB. The 3MB difference is irrelevant (user downloads one platform package). But the performance difference matters: tree-sitter index builds and scope graph queries are CPU-bound, and `opt-level = 3` makes them measurably faster. Keep LTO=fat, strip=true, codegen-units=1, panic=abort for size. Change opt-level to 3 for speed. Corrects the earlier research that specified opt-level=z.

### Decision: Platform packages only publish when core/ submodule changes — not on every omegon release

**Status:** decided
**Rationale:** Avoid empty Rust releases when only TypeScript changed. CI detects whether the core/ submodule SHA changed between releases. If unchanged, platform packages are not rebuilt or published — the meta-package's optionalDependencies point to the previous (still valid) platform package version. If changed, full matrix build + publish. This avoids version number churn on crates.io/npm while maintaining the single-product-version principle — the product version tracks the omegon npm package, but the platform binary packages only update when the binary actually changes.

### Decision: Windows is not a target — WSL users are served by linux-x64

**Status:** decided
**Rationale:** Omegon doesn't support native Windows (pi/Claude Code doesn't either). The upstream runtime requires Unix signal handling, process groups, and pty support. WSL users get the linux-x64 binary. Native Windows support would require significant work (terminal rendering, process management, path handling) for minimal audience. This can be revisited if demand materializes.

### Decision: Standalone install: binary from GitHub Release + `npm install -g @omegon/bridge` for LLM providers

**Status:** decided
**Rationale:** The Rust binary is 7.5MB and self-contained except for the LLM bridge (307 lines of JS that imports pi-ai for streaming provider clients). Rather than bundling Node.js or trying to eliminate it (Phase 3), the immediate install path is: (1) download the binary from GitHub Releases, (2) npm install -g @omegon/bridge which pulls in pi-ai and installs the bridge script. The binary auto-discovers the bridge via `which omegon-bridge` or the --bridge flag. This is two commands, clear separation of concerns, and works today without platform npm packages or CI build matrices. Path A (binary inside npm omegon package) remains the long-term distribution channel.

### Decision: Primary distribution is GitHub Releases (standalone binary), not npm — npm retained for transition only

**Status:** decided
**Rationale:** The E2E test proved the Rust binary is fully self-contained — native Anthropic + OpenAI clients, OAuth login, no Node.js dependency. npm distribution is now a supply chain liability, not a requirement. The primary install path is: download binary from GitHub Releases (7.7MB tarball), run `omegon-agent login` or set API key. npm platform packages are retained only for the transition period where TS Omegon coexists as the interactive mode. Supersedes the earlier decision about @omegon/bridge npm package (no longer needed).

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `core/Cargo.toml` (modified) — Add [profile.release] with LTO, strip, codegen-units=1, panic=abort, opt-level=3
- `npm/platform-packages/` (new) — Scaffold 4 platform package dirs with package.json templates
- `extensions/lib/omegon-subprocess.ts` (modified) — Add platform-package binary resolution path (node_modules/@styrene-lab/omegon-{platform})
- `.github/workflows/publish.yml` (modified) — Add Rust build matrix, platform package publish, core-changed detection
- `scripts/build-platform-packages.sh` (new) — Script to build + package platform binaries for local testing

### Constraints

- Use @styrene-lab/omegon-{platform} scope (existing trusted publisher)
- Platform packages only publish when core/ submodule changes
- Windows is not a target — WSL served by linux-x64
- Binary must include bundled sqlite (rusqlite bundled feature)
