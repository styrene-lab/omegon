+++
id = "38f1f0d4-ca4e-4e91-8df6-0113d2623d3d"
kind = "design_node"
title = "Defense-in-Depth with Sacrificial Fuse Points"
status = "exploring"
tags = ["security", "architecture", "resilience", "cleave", "openspec"]
aliases = ["fuse-aware-design"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
dependencies = []
open_questions = ["Should this become two nodes — one for the general 'fuse-aware design' philosophy (applicable to cleave, openspec, styrened, all systems) and one for the concrete web security implementation (the first application of the pattern)?", "What does a fuse declaration look like in practice? A section in OpenSpec specs ('Fuse Points')? An annotation in cleave plans? A first-class primitive in the design tree?", "How does fuse-aware design interact with cleave_assess? Should complexity scoring account for whether failure modes at system boundaries are defined vs. undefined? An undefined fuse point is hidden complexity."]
+++

# Defense-in-Depth with Sacrificial Fuse Points

## Overview

A systems design philosophy where every layer boundary is an engineered failure point — a mechanical fuse that breaks predictably under excess pressure, protecting downstream components and generating telemetry on trip.

Traditional defense-in-depth stacks protections but treats breaches as binary (held/failed). This design treats each layer boundary as a **sacrificial gear** — a component designed to shear cleanly at a known threshold rather than deform unpredictably. Every "accepted fallthrough" between layers is an engineered break point.

The key insight: **every system boundary is either an engineered fuse or an uncontrolled fracture line. There is no third option.**

This pattern applies beyond security — to CI/CD pipelines, mesh networking, task decomposition (cleave), and specification (OpenSpec). The web security use case (honeypots, tar pits, poison pills) is the first concrete application.

## Research

### The Sacrificial Fuse Model

In mechanical engineering, a shear pin or sacrificial gear is a component deliberately designed to fail at a known load threshold, protecting more expensive or critical components downstream. The fuse absorbs the overload energy and its failure mode is predictable — it shears cleanly rather than deforming unpredictably.

Applied to web security layers (first application — styrene.io):

**Layer** → **Normal function** → **Fuse behavior when bypassed** → **Signal generated**

1. **DNS/CDN (Cloudflare)** → geo-filtering, DDoS absorption → fallthrough exposes origin IP → alert on direct-to-origin traffic
2. **robots.txt / well-known** → polite bot guidance → fallthrough identifies bad actors → log bots that ignore disallow
3. **Honeypot links** → invisible to humans → fallthrough confirms automated scraper → immediate fingerprint + IP capture
4. **Tar pit endpoints** → waste scraper time/resources → fallthrough = scraper has resource budget → measure cost imposed
5. **Rate limiting (Envoy)** → throttle aggressive clients → fallthrough = distributed attack → alert on pattern shift
6. **Behavioral fingerprinting** → identify non-human patterns → fallthrough = sophisticated bot → escalate to challenge
7. **Poison pill content** → pollute scraped data → fallthrough = they got bad data → the data itself is the fuse
8. **OIDC auth boundary** → protect staging/sensitive content → fallthrough = credential compromise → session audit trail
9. **Content segmentation** → real content isolated from bait → fallthrough = nothing left to take → you're at the vault door

Each fuse trip is an **event** that feeds metrics. The chain isn't just defense — it's an instrumented kill chain in reverse, where each stage tells you more about the attacker's capabilities and intent.

Key principle: **a fuse that trips silently is worthless.** Every sacrificial point must emit telemetry.

### Generalizing Beyond Web Security — Fuse-Aware Design

The sacrificial fuse model isn't specific to security. It's a systems design primitive that applies everywhere pressure can exceed design parameters:

**In CI/CD pipelines:** Each stage is a fuse. Lint → test → build → deploy. But most pipelines treat failure as "stop." A fuse-aware pipeline would ask: when this stage fails, what controlled state do we land in? A failed deploy should leave the previous version running (it does, via ArgoCD), but a failed build should not leave a half-pushed image (does kaniko clean up on failure?). Each failure point should be explicitly designed, not just caught.

**In mesh networking (styrened):** When a Reticulum link degrades, what's the sacrificial layer? Is it latency (accept slower delivery), bandwidth (shed low-priority traffic), or availability (drop the link and re-announce)? Today this is implicit in RNS. Making it explicit means choosing which fuse blows first.

**In the cleave decomposition model:** When a child task fails, what breaks? Currently cleave reports the failure and the parent merge has a gap. A fuse-aware cleave would define: what's the minimum viable merge? Which children are load-bearing vs. decorative? A failed decorative child is a tripped fuse that doesn't compromise the structure.

**In OpenSpec:** Specs define what MUST be true. But specs could also define what's ALLOWED to fail — explicit "this scenario may degrade to X under Y conditions." That's a spec-level fuse definition.

### The Meta-Pattern

1. Identify every boundary where pressure transfers between components
2. At each boundary, define the failure mode explicitly (what breaks, what's preserved)
3. Instrument the break point (telemetry on trip)
4. Design the post-trip state (graceful degradation, not crash)
5. Test the fuse (chaos engineering, load testing, red team)

This is essentially what resilience engineering calls "graceful degradation" and what Nassim Taleb calls "antifragility" at the boundary layer — but with a mechanical engineering metaphor that's more precise. A fuse doesn't get stronger from stress (antifragile). It breaks predictably and protectably. The SYSTEM gets stronger because the fuse absorbed what would have been catastrophic.

### Potential Integration Points in Omegon

- **cleave_assess** — undefined failure modes at system boundaries increase complexity score (hidden risk)
- **cleave_run** — child tasks declare load-bearing vs. decorative; failed decorative children don't block merge
- **OpenSpec specs** — `## Fuse Points` section alongside `Given/When/Then` defining allowed degradation modes
- **design_tree** — nodes declare fuse relationships; `add_fuse_point` action for explicit boundary definition
- **skills** — a `fuse-aware-design` skill that prompts for failure mode definition at every system boundary

## Open Questions

- Should this become two nodes — one for the general 'fuse-aware design' philosophy (applicable to cleave, openspec, styrened, all systems) and one for the concrete web security implementation (the first application of the pattern)?
- What does a fuse declaration look like in practice? A section in OpenSpec specs ('Fuse Points')? An annotation in cleave plans? A first-class primitive in the design tree?
- How does fuse-aware design interact with cleave_assess? Should complexity scoring account for whether failure modes at system boundaries are defined vs. undefined? An undefined fuse point is hidden complexity.
