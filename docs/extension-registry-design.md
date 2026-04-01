# Extension Registry Design

## Overview

A distributed extension discovery and installation system for Omegon. Extensions can be:
1. **Curated** — published in `styrene-lab/omegon-extensions` registry
2. **Custom** — installed from any git URL (GitHub, GitLab, personal repos, etc.)
3. **Local** — developed locally in `~/.omegon/extensions/`

## Architecture

### Registry Structure

**styrenе-lab/omegon-extensions** (central repository)

```
omegon-extensions/
├── README.md                          # How to use the registry
├── registry.toml                      # Authoritative index of curated extensions
├── extensions/
│   ├── scribe-rpc/
│   │   ├── README.md                  # Extension description
│   │   ├── releases/                  # Links to official releases
│   │   │   ├── v0.1.0/
│   │   │   ├── v0.2.0/
│   │   └── manifest-schema.md         # Validated manifest
│   ├── python-analyzer/
│   │   ├── README.md
│   │   ├── releases/
│   │   └── manifest-schema.md
│   └── ...
├── docs/
│   ├── publishing.md                  # How to publish an extension
│   ├── standards.md                   # Quality/safety standards
│   └── curation-process.md
└── templates/
    ├── rust-extension/
    ├── python-extension/
    └── typescript-extension/
```

### Registry Index Format

**registry.toml:**

```toml
# Published extensions (curated)
[[extension]]
name = "scribe-rpc"
description = "Engagement tracking and timeline"
author = "Styrene Lab"
repository = "https://github.com/styrene-lab/scribe-rpc"
latest_version = "0.2.0"
sdk_version_constraint = "0.15"

[[extension]]
name = "python-analyzer"
description = "Python code analysis"
author = "Community"
repository = "https://github.com/user/python-analyzer"
latest_version = "0.1.0"
sdk_version_constraint = "0.15"
```

## Installation Methods

### 1. From Registry

```bash
omegon install scribe-rpc           # Latest version
omegon install scribe-rpc@0.1.0     # Specific version
omegon install scribe-rpc@latest    # Explicit latest
```

Omegon:
1. Looks up extension in `registry.toml` from styrene-lab/omegon-extensions
2. Fetches manifest from registry
3. Validates SDK version constraint
4. Downloads binary from GitHub releases
5. Installs to `~/.omegon/extensions/scribe-rpc/`

### 2. From Git URL

```bash
omegon install git:https://github.com/user/my-extension
omegon install git:user/my-extension                    # GitHub shorthand
omegon install git:gitlab.com/user/my-extension         # GitLab
omegon install git:https://github.com/user/my-extension@v0.2.0
```

Omegon:
1. Clones repository to temp directory
2. Looks for manifest.toml in root or `extensions/{name}/` or `.omegon/extensions/{name}/`
3. Validates SDK version
4. Builds binary (if native extension)
5. Installs to `~/.omegon/extensions/{name}/`

### 3. Local Development

```bash
# Manual: just place repo in ~/.omegon/extensions/my-extension/
cd ~/.omegon/extensions
git clone https://github.com/user/my-extension
cd my-extension
cargo build --release
```

Omegon auto-discovers on next TUI restart.

## Git URL Extension Format

For custom extensions installed via `git:`, the repository structure can be:

**Flat (extension at root):**
```
user/my-extension/
├── Cargo.toml
├── manifest.toml
├── src/main.rs
└── README.md
```

**Nested (in extensions/ subdirectory):**
```
user/monorepo/
├── extensions/
│   └── my-extension/
│       ├── Cargo.toml
│       ├── manifest.toml
│       ├── src/main.rs
│       └── README.md
└── other-projects/
```

**Bundled (.omegon/extensions/):**
```
user/my-extension/
├── .omegon/
│   └── extensions/
│       └── my-extension/
│           ├── Cargo.toml
│           ├── manifest.toml
│           └── src/
└── README.md
```

## Installation Flow

```
User: omegon install scribe-rpc
           ↓
   Query registry.toml
           ↓
   Found: scribe-rpc v0.2.0
           ↓
   Check SDK version (0.15 constraint ✓)
           ↓
   Download binary from GitHub release
           ↓
   Verify manifest.toml
           ↓
   Install to ~/.omegon/extensions/scribe-rpc/
           ↓
   Health check: call ping_method
           ↓
   Success: ready for next TUI start
```

## Extension Publishing

### For Registry (Curated)

1. **Create PR** to `styrene-lab/omegon-extensions`
   - Add entry to `registry.toml`
   - Add `extensions/{name}/README.md`
   - Link to your GitHub releases

2. **Community review**
   - Security audit
   - Code quality check
   - SDK version validation

3. **Merge** to registry
   - Now discoverable via `omegon install {name}`

### For Custom (Self-Hosted)

1. Build extension with `omegon-extension` SDK
2. Push to GitHub (public or private)
3. Create GitHub releases with built binaries
4. Share git URL: `omegon install git:user/my-extension`

## Version Management

### Extension Versions

Each extension has a semantic version in `Cargo.toml`:
```toml
[package]
version = "0.2.0"

[dependencies]
omegon-extension = "0.15.6"
```

When publishing GitHub release, tag as `v0.2.0`. Users can install specific versions:
```bash
omegon install scribe-rpc@0.2.0
```

### SDK Version Constraints

Extensions declare their SDK version in manifest:
```toml
[extension]
sdk_version = "0.15"
```

At install time:
- `sdk_version = "0.15"` matches Omegon SDK `0.15.0`, `0.15.6`, `0.15.6-rc.1` (prefix match)
- Mismatch → installation fails with clear error
- `omegon` binary reports its SDK version: `omegon --version` (includes SDK version)

## Updating Extensions

```bash
omegon update scribe-rpc              # Update to latest
omegon update scribe-rpc@0.2.0        # Update to specific version
omegon update                         # Update all
```

Omegon:
1. Checks for newer version (registry or git)
2. Downloads new binary
3. Stops old process gracefully (SIGTERM + 5s timeout)
4. Installs new binary
5. Health check with new version
6. Next TUI start uses new extension

## Uninstalling

```bash
omegon uninstall scribe-rpc           # Remove from ~/.omegon/extensions/
```

## TUI Integration

### Extension Management UI

Add to Omegon TUI (e.g., `/extensions` command or Alt+E):

```
Installed Extensions (3):
  scribe-rpc (v0.2.0) [registry]      UPDATE AVAILABLE (v0.3.0)
  python-analyzer (v0.1.0) [git]      ✓ up-to-date
  my-local-ext (dev) [local]          ⚠ not in registry

Browse Registry:
  python-analyzer
  code-review-bot
  database-explorer
  (20 more)
```

Features:
- List installed with versions and sources
- Show available updates
- Browse registry
- Install/update/uninstall from TUI
- View extension details (description, author, docs)

## CLI Commands

```bash
# Install
omegon install <name>[@version]                # Registry
omegon install git:<url>[@version]             # Git URL
omegon install git:<shorthand>[@version]       # GitHub shorthand

# Manage
omegon extension list                          # List installed
omegon extension info <name>                   # Details
omegon extension search <query>                # Search registry
omegon extension update [name]                 # Update
omegon extension uninstall <name>              # Remove

# Development
omegon extension dev <path>                    # Mount local extension
omegon extension reload [name]                 # Hot-reload (future)

# Publishing
omegon extension publish                       # Prepare for registry submission
omegon extension validate <path>               # Validate extension
```

## Safety & Security

### Install-Time Validation

- ✅ Manifest schema validation
- ✅ SDK version compatibility check
- ✅ Binary signature verification (future: GPG)
- ✅ Sandboxed test run (health check)

### Runtime Safety

- ✅ Process isolation (RPC over stdin/stdout)
- ✅ Timeout enforcement on all RPC calls
- ✅ Resource limits (future: memory, CPU)

### Registry Curation

- ✅ Manual code review before publication
- ✅ Version constraints prevent breaking changes
- ✅ Security advisories (future: vulnerability DB)
- ✅ Extension can be yanked if unsafe discovered

## Roadmap

### Phase 1 (0.15.6+)
- [x] SDK crate (omegon-extension)
- [ ] Registry index (registry.toml)
- [ ] Git URL installation
- [ ] `omegon install` command

### Phase 2 (0.16)
- [ ] TUI integration (/extensions command)
- [ ] Extension update mechanism
- [ ] Registry web interface
- [ ] Security audit process

### Phase 3 (0.17+)
- [ ] Hot-reload for development
- [ ] Multi-language SDKs (Python, Go, TypeScript)
- [ ] Shared dependencies
- [ ] Resource limits & sandboxing
- [ ] GPG signatures

## Related Design Nodes

- `extension-template-generator` — scaffold new extensions
- `extension-hot-reload` — development workflow
- `multi-language-extension-sdks` — Python, Go, TypeScript SDKs
