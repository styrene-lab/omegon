+++
id = "a12ee5a7-d244-4b57-b284-41fd280d5556"
kind = "document"
title = "mdserve: Nix flake + packaging"
status = "exploring"
tags = ["nix", "distribution", "packaging", "rust"]
aliases = ["mdserve-nix-distribution"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
dependencies = ["mdserve-dioxus-frontend"]
issue_type = "chore"
open_questions = ["What is the actual packaging goal for v1: reproducible developer install for the mdserve/Auspex daemon only, or end-user distribution with browser assets and runtime dependencies fully bundled? Those are different problems.", "How should frontend assets be packaged in Nix builds once the portal stack is chosen: compiled into the Rust binary, installed alongside it in the store, or generated as a separate derivation consumed by the binary?", "What operator entrypoint should Omegon/Auspex assume for starting the daemon: PATH-discoverable binary, `nix run`, or dev-shell-only workflow? The bridge UX depends on this contract."]
parent = "markdown-viewport"
related = []
+++

# mdserve: Nix flake + packaging

## Overview

Nix flake for the mdserve fork following the styrened pattern. flake-utils.lib.eachDefaultSystem, buildRustPackage (or crane for incremental builds), version from VERSION file, commitSha injection. Must handle the two-step build: WASM bundle (dioxus-cli + wasm-opt) as a separate derivation, embedded in main binary via include_bytes!. Dev shell with cargo, rust-analyzer, dioxus-cli, wasm-pack, cargo-watch. Platforms: aarch64-darwin, x86_64-darwin, x86_64-linux, aarch64-linux. Should wait until the Dioxus WASM build shape stabilizes before finalizing the derivation structure.

## Decisions

### Decision: Nix/distribution work follows backend and frontend shape decisions

**Status:** decided

**Rationale:** Packaging should reflect the chosen runtime architecture rather than drive it. Until the backend serving model and frontend asset strategy are concrete, distribution work risks optimizing the wrong artifact boundary.

## Open Questions

- What is the actual packaging goal for v1: reproducible developer install for the mdserve/Auspex daemon only, or end-user distribution with browser assets and runtime dependencies fully bundled? Those are different problems.
- How should frontend assets be packaged in Nix builds once the portal stack is chosen: compiled into the Rust binary, installed alongside it in the store, or generated as a separate derivation consumed by the binary?
- What operator entrypoint should Omegon/Auspex assume for starting the daemon: PATH-discoverable binary, `nix run`, or dev-shell-only workflow? The bridge UX depends on this contract.
