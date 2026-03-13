---
id: mdserve-nix-distribution
title: "mdserve: Nix flake + packaging"
status: seed
parent: markdown-viewport
dependencies: [mdserve-dioxus-frontend]
tags: [nix, distribution, packaging, rust]
open_questions: []
issue_type: chore
---

# mdserve: Nix flake + packaging

## Overview

Nix flake for the mdserve fork following the styrened pattern. flake-utils.lib.eachDefaultSystem, buildRustPackage (or crane for incremental builds), version from VERSION file, commitSha injection. Must handle the two-step build: WASM bundle (dioxus-cli + wasm-opt) as a separate derivation, embedded in main binary via include_bytes!. Dev shell with cargo, rust-analyzer, dioxus-cli, wasm-pack, cargo-watch. Platforms: aarch64-darwin, x86_64-darwin, x86_64-linux, aarch64-linux. Should wait until the Dioxus WASM build shape stabilizes before finalizing the derivation structure.

## Open Questions

*No open questions.*
