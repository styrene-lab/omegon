+++
id = "0e5685f9-3cae-4575-80ef-adcb3f98426b"
kind = "document"
title = "Omegon Installation & Distribution"
status = "implemented"
tags = ["distribution", "dx", "packaging"]
aliases = ["omegon-install"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
+++

# Omegon Installation & Distribution

## Overview

Engineers should be able to install Omegon with a single command: no git clone, no submodule init, no npm runtime, and no manual link step. The current supported product boundary is the Rust binary plus bundled skills/catalog assets.

Current install surfaces:

- install script: `curl -fsSL https://omegon.styrene.io/install.sh | sh`
- nightlies: `curl -fsSL https://omegon.styrene.io/install.sh | sh -s -- --channel=nightly`
- Homebrew: `brew tap styrene-lab/tap && brew install omegon`
- direct GitHub release artifacts from `styrene-lab/omegon`

Source checkouts use `just build` and `just link`. `just link` installs the stable development launcher into `~/.local/bin/omegon` and `~/.local/bin/om`, registers the checkout in `~/.omegon/channels/default`, and keeps a fallback copy in `~/.omegon/bin/omegon`. It does not use shell-profile aliases as the primary resolution mechanism; run `omegon --which` to inspect the resolved target.

## Linux runtime requirements

**Important:** Homebrew on Linux does **not** solve host glibc ABI compatibility for Omegon release binaries.

If a Linux release artifact was built against a newer glibc than your distro provides, install may succeed but the binary will fail immediately at runtime with errors like:

```text
omegon: /lib/x86_64-linux-gnu/libc.so.6: version `GLIBC_2.38' not found
omegon: /lib/x86_64-linux-gnu/libc.so.6: version `GLIBC_2.39' not found
```

That means the host system glibc is older than the binary expects.

### Current expectation

Before relying on a Linux Homebrew install, verify that your host glibc is new enough for the shipped release artifact:

```bash
ldd --version
```

If your system glibc is older than the required version for the current release artifact, `brew install` alone is not sufficient.

### What to do if this happens

Use one of these paths:

- run Omegon on a newer Linux distribution with a compatible glibc
- use a container/VM image that provides the required glibc baseline
- use another distribution channel once an older-glibc or musl/static Linux artifact is published

### Documentation contract

Linux install surfaces must state runtime ABI requirements explicitly. `brew install` should never imply that Homebrew will supply a compatible glibc for Omegon binaries on Linux.

## Update contract

Omegon is the single installed product boundary. There is no Node.js runtime package or companion TypeScript fork to update separately.

The authoritative update path therefore must:
- mutate the installed runtime surface (`/update install`, `brew upgrade omegon`, or reinstall via `install.sh` depending on channel)
- verify the active `omegon` / `om` executable still resolves to Omegon
- stop at a deliberate restart handoff that tells the operator to relaunch `om` or `omegon`

`/refresh` is intentionally narrower: it only clears transient caches and reloads extensions. It is not equivalent to `/update` after package/runtime mutation.

## Decisions

### Decision: Rust binary is the product boundary

**Status:** implemented
**Rationale:** The installable product is the Rust binary plus bundled assets. This avoids Node/npm runtime dependency drift, submodule packaging failures, and multi-product update ambiguity.

### Decision: Release artifacts are CI-owned

**Status:** implemented
**Rationale:** Operator workstations may build and sign local validation binaries, but distributable archives, checksums, signatures, attestations, Homebrew updates, and site deployments should come from CI.

### Decision: Repo under styrene-lab GitHub org

**Status:** decided
**Rationale:** The canonical upstream is `styrene-lab/omegon`. Install, update, release, and docs links should use that owner.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `site/src/pages/docs/install.astro` — public install docs
- `site/snippets/install.yaml` — canonical install commands
- `Justfile` — source build, validation, and local link recipes
- `.github/workflows/*` — CI release/site artifact production
- `homebrew/` — Homebrew packaging metadata

### Constraints

- Linux artifacts must state their glibc baseline clearly.
- Homebrew-managed installs should update through Homebrew.
- Script-managed installs should update by rerunning the install script or using `/update`.
- Source checkout development should use `just build` and `just link`.

## Migration note

Older TypeScript/npm/pi distribution notes are historical only. New docs, scripts, and release automation should describe the Rust binary install boundary.
