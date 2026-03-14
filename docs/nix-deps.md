---
id: nix-deps
title: Nix as Unified Dependency Manager
status: exploring
tags: [infra, bootstrap, cross-platform]
open_questions:
  - "Should Nix be the only install path, or a preferred-first with brew/apt fallback?"
  - "Flake-based (nix develop) vs nix-env/nix profile install for end users — which UX model?"
  - "How does bootstrap detect and offer Nix installation if Nix itself isn't present?"
  - "Can Nix manage ollama (daemon/service) or only CLI tools? What about GPU-dependent deps?"
  - Does Nix work well on macOS (Apple Silicon) for all our deps — d2, pandoc, gh, poppler, librsvg?
---

# Nix as Unified Dependency Manager

## Overview

Explore using Nix as omegon's preferred package manager for external dependencies (ollama, d2, pandoc, gh, etc.). Nix provides deterministic, reproducible builds across Linux and macOS without requiring root access — eliminating the current matrix of brew/apt/dnf/rpm-ostree install commands and the per-distro edge cases (e.g. Bazzite aliasing dnf to documentation guides).\n\nA nix flake or shell.nix could declare all external deps in one place, making `nix develop` or `nix profile install` the single install command across every platform.

## Open Questions

- Should Nix be the only install path, or a preferred-first with brew/apt fallback?
- Flake-based (nix develop) vs nix-env/nix profile install for end users — which UX model?
- How does bootstrap detect and offer Nix installation if Nix itself isn't present?
- Can Nix manage ollama (daemon/service) or only CLI tools? What about GPU-dependent deps?
- Does Nix work well on macOS (Apple Silicon) for all our deps — d2, pandoc, gh, poppler, librsvg?
