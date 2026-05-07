+++
id = "a5f6ec37-df0f-4034-b14a-7233aa1975e4"
kind = "design_node"
title = "mdserve: Nix flake + packaging"
status = "seed"
tags = ["nix", "distribution", "packaging", "rust"]
aliases = ["mdserve-nix-distribution"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
dependencies = ["mdserve-dioxus-frontend"]
issue_type = "chore"
open_questions = []
parent = "markdown-viewport"
+++

# mdserve: Nix flake + packaging

## Overview

Nix flake for the mdserve fork following the styrened pattern. flake-utils.lib.eachDefaultSystem, buildRustPackage (or crane for incremental builds), version from VERSION file, commitSha injection. Must handle the two-step build: WASM bundle (dioxus-cli + wasm-opt) as a separate derivation, embedded in main binary via include_bytes!. Dev shell with cargo, rust-analyzer, dioxus-cli, wasm-pack, cargo-watch. Platforms: aarch64-darwin, x86_64-darwin, x86_64-linux, aarch64-linux. Should wait until the Dioxus WASM build shape stabilizes before finalizing the derivation structure.

## Open Questions

*No open questions.*
