+++
id = "4d08f54f-a133-4b07-adb6-9f068ab8fc16"
kind = "document"
title = "Update lifecycle parity and restart boundary"
status = "archived"
tags = []
aliases = ["update-lifecycle-parity"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
archive_reason = "obsolete"
archived_at = "1775246679"
dependencies = []
open_questions = []
parent = "singular-package-update-lifecycle"
related = []
superseded_by = "rust-agent-loop"
+++

# Update lifecycle parity and restart boundary

## Overview

> Parent: [Singular package integration and full-lifecycle update parity](singular-package-update-lifecycle.md)
> Spawned from: "What exact steps define full-lifecycle update parity relative to `./scripts/install-pi.sh`, and which of those steps can be safely executed in-process versus requiring a restart/reexec boundary?"

*To be explored.*

## Research

### Current /update behavior falls short of install-pi parity in dev mode

`extensions/bootstrap/index.ts` dev-mode `/update` currently performs `git pull --ff-only`, `git submodule update --init --recursive`, `npm run build` in `vendor/pi-mono`, root `npm install --install-links=false`, clears the jiti cache, and calls `ctx.reload()`. By contrast `scripts/install-pi.sh` builds pi-mono, runs `npm link --force` at the Omegon root, and verifies that `which pi` resolves to Omegon. Therefore current `/update` does not relink the active global binary, does not verify the binary target, and assumes hot reload is sufficient after replacing core/runtime files.

### Installed-mode update already uses a restart boundary

The former installed-mode updater explicitly stopped at a restart boundary rather than attempting in-process reload after replacing runtime files. That remains the relevant safety contract: package mutation and process restart are separate operations.

### Current live binary target can be verified after link/install

On this machine `which pi` resolves to `/opt/homebrew/bin/pi`, which is a symlink to `../lib/node_modules/omegon/bin/pi.mjs`. This proves the active binary path can be verified after update/link operations, and suggests that true parity should include a post-update check that the resolved binary still points at Omegon rather than assuming npm/link state is correct.

## Decisions

### Decision: Full-lifecycle parity should include relink/verification, but end at a deliberate restart boundary rather than in-process reexec

**Status:** decided

**Rationale:** Replacing Omegon's package files and forked pi runtime mutates the code currently executing the session. Installed mode already treats restart as the safe contract. Dev mode should become functionally equivalent to `./scripts/install-pi.sh` by pulling, syncing submodule, rebuilding, refreshing dependencies, relinking/verifying the active `pi` binary, and clearing caches, but it should then stop and instruct the operator to restart instead of attempting `ctx.reload()` inside the old process.

### Decision: Reload remains a lightweight cache refresh path, not the authoritative full update handoff

**Status:** decided

**Rationale:** `/refresh` or reload-like behavior is still useful for extension-only or transpilation-cache changes, but it is not semantically equivalent to reinstalling/relinking Omegon. Keeping `/refresh` lightweight avoids conflating hot-reload convenience with package lifecycle mutation.
