---
id: bundled-armory-local-skill-posture
title: "Bundled vs Armory vs Local Skill Posture"
status: seed
tags: [skills, armory, release-0.27.0]
open_questions:
  - "[assumption] Existing bundled skills remain in-source for 0.27.0; deeper reclassification into bundled vs Armory vs local is deferred until after release hardening."
  - "[assumption] Pre-0.27.0 skill work should be limited to polish and correctness of current bundled skill text, not substrate or activation architecture changes."
  - "[assumption] The Flynt skill should remain the docs-profile markdown default even though its project signals are broad (`*.md`, `docs/**/*.md`); whether it should eventually split into generic markdown vs Flynt-specific guidance remains unresolved."
dependencies: []
related: []
---

# Bundled vs Armory vs Local Skill Posture

## Overview

Future design exploration for deciding which skills should ship inside the Omegon binary, which should live in Armory/catalog distribution, and which should remain local/project-specific. Open after 0.27.0 release hardening; current pre-0.27.0 work is limited to polishing the existing bundled skill set without reorganizing the substrate.

## Research

### Pre-0.27.0 bundled skill inventory assessment

Current bundled skills inventory: code-act, git, oci, openspec, python, rust, security, style, typescript, flynt. Potential future posture split: core harness skills vs standard-library domain/language skills vs project/brand-local skills. Pre-0.27.0 polish addressed low-risk issues in the existing bundle: removed a Zellij-specific sentence from the generic Rust skill, aligned the OCI CI example with the guidance to avoid `latest`, softened Python environment-manager defaults, toned down brand-specific style language, removed a legacy package-name reference from the TypeScript skill, and renamed the former `vault` markdown skill to `flynt` to avoid overloading Vault/security terminology.

## Open Questions

- [assumption] Existing bundled skills remain in-source for 0.27.0; deeper reclassification into bundled vs Armory vs local is deferred until after release hardening.
- [assumption] Pre-0.27.0 skill work should be limited to polish and correctness of current bundled skill text, not substrate or activation architecture changes.
- [assumption] The Flynt skill should remain the docs-profile markdown default even though its project signals are broad (`*.md`, `docs/**/*.md`); whether it should eventually split into generic markdown vs Flynt-specific guidance remains unresolved.
