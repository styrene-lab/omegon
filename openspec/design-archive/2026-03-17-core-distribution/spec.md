+++
id = "187fad9e-318c-48ac-855e-dc08c7127920"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Core binary distribution — shipping Rust alongside and eventually instead of TypeScript — Design Spec (extracted)

> Auto-extracted from docs/core-distribution.md at decide-time.

## Decisions

### Platform-specific npm packages for the Rust binary (esbuild/swc model) (decided)

The `omegon` npm meta-package declares platform-specific packages (@omegon/darwin-arm64, @omegon/darwin-x64, @omegon/linux-x64, @omegon/linux-arm64) as optionalDependencies. npm installs only the matching platform. A resolver module finds the binary at runtime. This is the proven model used by esbuild, swc, turbo, Biome, and oxlint. Single `npm install -g omegon` gives users both the TypeScript runtime and the Rust binary. No separate install step, no postinstall downloads, no Rust toolchain required. Platform packages are ~3-8MB each (release + LTO + strip).

### Single version number across omegon and omegon-core — the product version is the npm version (decided)

One version for the product. omegon package.json version is authoritative. omegon-core Cargo.toml tracks it. Both repos are tagged with the same version on release. The submodule pin tracks which core version is integrated. CI builds both and publishes atomically from the omegon repo. Phase 1 (process inversion) is the 1.0.0 major version bump — the entry point changes from Node.js to Rust. Pre-1.0 versions are the Ship of Theseus transition.

### CI builds Rust binary from the core/ submodule as part of the omegon publish workflow (decided)

The omegon repo's publish.yml workflow gains a matrix job that builds omegon-core for 4 platforms (darwin-arm64, darwin-x64, linux-x64, linux-arm64), packages each into a platform npm package, and publishes them before the meta-package. This is atomic — all platform packages and the meta-package share the same version and publish in the same CI run. Build optimization: release profile with LTO=fat, strip=true, codegen-units=1, opt-level=z, panic=abort. sqlite is bundled (no system dependency). Expected binary size: 3-8MB per platform.

### GitHub Releases as parallel standalone channel from Phase 0 onward (decided)

Every release publishes platform tarballs as GitHub Release assets alongside the npm packages. This serves: k8s pod deployments (no Node.js needed in the image), CI environments, and users who want just the binary. During Phase 0-1, standalone misses TypeScript features. From Phase 2+, standalone is fully functional. Homebrew formula can be added when demand warrants. npm remains the primary channel throughout the transition.

### Use @omegon/ scope if registerable, fall back to @styrene-lab/omegon-* prefix (decided)

The @omegon/ scope is cleaner (@omegon/darwin-arm64 vs @styrene-lab/omegon-darwin-arm64). npm org registration is free for public packages. If the omegon org name is taken or unavailable, fall back to @styrene-lab/ with an omegon- prefix. Either way the meta-package stays as unscoped `omegon`. This is a cosmetic choice that doesn't affect architecture — resolve it when setting up the first platform package publish.

### opt-level = 3 (speed) over opt-level = z (size) — tree-sitter parsing performance matters more than binary size (decided)

The understand tool's tree-sitter grammars add 8-15MB to the binary. With `opt-level = "z"` (size) the binary is ~10-15MB; with `opt-level = 3` (speed) it's ~13-18MB. The 3MB difference is irrelevant (user downloads one platform package). But the performance difference matters: tree-sitter index builds and scope graph queries are CPU-bound, and `opt-level = 3` makes them measurably faster. Keep LTO=fat, strip=true, codegen-units=1, panic=abort for size. Change opt-level to 3 for speed. Corrects the earlier research that specified opt-level=z.

### Platform packages only publish when core/ submodule changes — not on every omegon release (decided)

Avoid empty Rust releases when only TypeScript changed. CI detects whether the core/ submodule SHA changed between releases. If unchanged, platform packages are not rebuilt or published — the meta-package's optionalDependencies point to the previous (still valid) platform package version. If changed, full matrix build + publish. This avoids version number churn on crates.io/npm while maintaining the single-product-version principle — the product version tracks the omegon npm package, but the platform binary packages only update when the binary actually changes.

### Windows is not a target — WSL users are served by linux-x64 (decided)

Omegon doesn't support native Windows (pi/Claude Code doesn't either). The upstream runtime requires Unix signal handling, process groups, and pty support. WSL users get the linux-x64 binary. Native Windows support would require significant work (terminal rendering, process management, path handling) for minimal audience. This can be revisited if demand materializes.

## Research Summary

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
    "@omegon/darwin-…

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
- Network dependency during insta…

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
- Users must install separately from the npm package (dur…

### What fits the Ship of Theseus transition

During the transition, both TypeScript and Rust need to be distributed *together*. The user does `npm install -g omegon` and gets both — the TS runtime for the interactive session and the Rust binary for cleave children (Phase 0) and eventually as the main entry point (Phase 1+).

The **esbuild/swc model** is the right fit:
- Single install command
- The npm package is already the distribution channel
- Platform binaries are bundled automatically
- When the Rust binary becomes the entry point (P…

### Versioning strategy — one version, two repos, converging



### The problem

Today: `omegon` npm package has version 0.10.7. The `omegon-core` Rust repo is at 0.1.0. They're different projects with different version numbers. But they're converging — eventually `omegon-core` IS `omegon`.

### The solution: omegon version is the product version, core follows

**One version number for the product.** The `omegon` npm package.json version is the authoritative version. The `omegon-core` Cargo.toml version tracks it. When omegon bumps to 0.11.0, omegon-core bumps to 0.11.0. They're the same product.

**How this works in practice:**

1. Development happens in both repos. omegon (TypeScript) and omegon-core (Rust) are developed in parallel.

2. When a release is cut, both repos are tagged with the same version:
   - `omegon` repo: `v0.11.0` → npm publish
  …

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

This is a single CI workf…

### Semantic versioning during the transition

The version communicates what changed:

- **Patch** (0.10.x): Bug fixes in either TypeScript or Rust. No new Phase 0 capabilities.
- **Minor** (0.x.0): New features, new lifecycle phases, new tools in the Rust core.
- **Major** (x.0.0): Breaking changes. Phase 1 (process inversion) is a major version bump — the binary entry point changes.

**Phase milestone versions:**
- 0.11.0: Phase 0 — Rust binary ships alongside, used for cleave children
- 0.12.0-0.x.0: Rust core gains tools, lifecycle engin…

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
1. `cargo build --release …

### Binary size optimization

```toml
[profile.release]
lto = "fat"        # link-time optimization across all crates
strip = true       # strip debug symbols
codegen-units = 1  # maximize optimization (slower build, smaller binary)
opt-level = "z"    # optimize for size over speed
panic = "abort"    # no unwinding machinery
```

Expected size: 3-8MB per platform. Compared to the current 191MB npm package, this is a 95%+ reduction for the core binary. The full npm package will still be large during the transition (TypeScript…

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

The binary is present but only used internally — the cleav…

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
co…

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
# Standalone inst…

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

With 5+ grammars the binary is more likely **10-20MB**, not 3-8MB. Still…

### 2. macOS x86_64 runner deprecation

GitHub deprecated `macos-13` (the last x86 runner) and `macos-14+` are all ARM. Cross-compilation from ARM to x86 is straightforward: `rustup target add x86_64-apple-darwin && cargo build --target x86_64-apple-darwin`. No dedicated x86 runner needed. The build matrix should use `macos-latest` for both macOS targets.

### 3. Empty core releases when only TypeScript changes

The "single version" approach means every omegon version bump also bumps omegon-core, even if the Rust code didn't change. This creates empty releases — version bumps with no code change.

**Refined approach:** Platform packages are only *published* when the core/ submodule actually changed. The meta-package always publishes. If core didn't change, the optionalDependencies point to the previous platform package version (which is still valid — npm doesn't require exact match for optionalDeps).

`…

### 4. No Windows target — intentional and should be explicit

The matrix has 4 targets, all Unix. This is correct — Omegon doesn't support Windows today (pi/Claude Code doesn't either). But the decision should be explicit: "Windows is not a target. WSL users are served by the linux-x64 binary."

### 5. OIDC trusted publishing per platform package

Each @omegon/ platform package needs its own npm trusted publisher configuration. That's 4 packages × 1 GitHub Actions workflow. This is a one-time setup but it's 4 separate npm org → GitHub repo connections. If we use @styrene-lab/ scope instead, the existing trusted publisher might cover all packages under that scope. Worth testing before committing to @omegon/ scope.

### 6. The Phase 1 wrapper is correct but should forward signals

`execFileSync(bin, args, { stdio: 'inherit' })` works for launching the Rust binary and inheriting terminal I/O. It also blocks the Node process (which is what we want — Node is just the launcher). Signal forwarding (SIGINT, SIGTERM) to the child works correctly with `stdio: 'inherit'` because the child is in the same process group. No changes needed.
