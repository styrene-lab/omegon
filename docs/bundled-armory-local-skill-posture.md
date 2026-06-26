---
id: bundled-armory-local-skill-posture
title: "Bundled vs Armory vs Local Skill Posture"
status: seed
tags: [skills, armory, release-0.27.0]
open_questions:
  - "[assumption] Existing bundled skills remain in-source for 0.27.0; deeper reclassification into bundled vs Armory vs local is deferred until after release hardening."
  - "[assumption] Pre-0.27.0 skill work should be limited to polish and correctness of current bundled skill text, not substrate or activation architecture changes."
dependencies: []
related: []
---

# Bundled vs Armory vs Local Skill Posture

## Overview

Future design exploration for deciding which skills should ship inside the Omegon binary, which should live in Armory/catalog distribution, and which should remain local/project-specific. Open after 0.27.0 release hardening; current pre-0.27.0 work is limited to polishing the existing bundled skill set without reorganizing the substrate.

## Research

### Pre-0.27.0 bundled skill inventory assessment

Current bundled skills inventory: code-act, git, oci, openspec, python, rust, security, style, typescript, vault. Potential future posture split: core harness skills vs standard-library domain/language skills vs project/brand-local skills. Pre-0.27.0 assessment found likely low-risk polish candidates: rust contains a Zellij-specific sentence in the generic skill; oci CI example pushes latest despite warning to avoid latest; python states no poetry/no conda too absolutely for a bundled generic skill; style contains brand-specific/flavor language; typescript mentions legacy pi-* package names in an illustrative SDK warning.

## Open Questions

- [assumption] Existing bundled skills remain in-source for 0.27.0; deeper reclassification into bundled vs Armory vs local is deferred until after release hardening.
- [assumption] Pre-0.27.0 skill work should be limited to polish and correctness of current bundled skill text, not substrate or activation architecture changes.
