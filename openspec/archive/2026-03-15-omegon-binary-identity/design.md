+++
id = "2ca485ae-aa7b-460e-8357-a2a2bfd4bef7"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Omegon binary identity — eliminate direct product exposure as `pi` — Design

## Architecture Decisions

### Decision: The Omegon happy path must be product-native; direct operator exposure to `pi` is a bug, not a branding detail

**Status:** decided
**Rationale:** If the operator still sees or is instructed to invoke `pi` directly, the singular-package model is incomplete. The owning product boundary is Omegon, so binary naming, update/restart language, docs, status messages, and install surface must all default to Omegon-native terminology and invocation. Any remaining `pi` entrypoint should be treated as an internal compatibility layer or legacy alias, not the primary product interface.

### Decision: Omegon must own the executable lifecycle boundary; direct `pi` invocation is an unsupported bypass of product control

**Status:** decided
**Rationale:** Once Omegon subsumed pi, it became responsible for startup policy, update/restart handoff, verification, extension loading, compatibility shims, and any future orchestration/runtime hooks. If the operator invokes `pi` directly, they can bypass the very boundary where Omegon is supposed to enforce that lifecycle. The migration goal is therefore not just 'show Omegon branding' but 'ensure the operator enters through the Omegon-controlled executable path every time.'

### Decision: `omegon` becomes the authoritative operator executable; `pi` is compatibility-only and must immediately re-enter the Omegon-controlled entrypoint

**Status:** decided
**Rationale:** Lifecycle ownership requires a single operator-facing executable boundary that Omegon owns end-to-end. The package should expose `omegon` as the canonical command, docs and prompts must instruct operators to launch and restart `omegon`, and verification helpers should validate Omegon ownership through that path. A temporary `pi` alias may remain only to preserve compatibility, but it must route to the exact same Omegon entrypoint and must not be treated as the normal or documented path.

### Decision: Executable verification and restart handoff must be expressed in terms of Omegon ownership, not merely successful `pi --where` probing

**Status:** decided
**Rationale:** The existing `--where` probe is useful, but as long as it is framed only as a `pi` check, the lifecycle contract is anchored to the compatibility alias instead of the product boundary. Verification should confirm that the active operator path is Omegon-owned, that the resolved runtime root is Omegon, and that restart/update instructions tell the operator to relaunch Omegon. Compatibility probing for `pi` may remain as a secondary sanity check during migration, but it cannot define correctness.

## Research Context

### Lifecycle ownership, not branding, is the primary driver

The operator concern is not merely that `pi` is the wrong product name. Because Omegon has subsumed pi as an implementation substrate, direct invocation of `pi` bypasses the lifecycle boundary where Omegon can enforce its own startup checks, update semantics, runtime verification, capability wiring, and future orchestration behavior. An operator-visible `pi` happy path therefore represents a lifecycle escape hatch, not just a branding inconsistency.

### Current lifecycle still proves Omegon through a `pi` alias, which leaves the product boundary porous

The current implementation already maps both `omegon` and `pi` to `bin/pi.mjs` in package.json, but the lifecycle language, verification, and helper contracts are still centered on the `pi` alias. README instructs operators to start with `pi`; `scripts/install-pi.sh` verifies `which pi` and `pi --where`; `/update` in extensions/bootstrap/index.ts inspects only the active `pi` path and ends with restart instructions that say `/exit, then pi`. This means the runtime ownership work is present, but the user-entered lifecycle boundary is still modeled as `pi` rather than an Omegon-owned executable contract.

## File Changes

- `package.json` (modified) — Promote `omegon` as the canonical bin surface and decide whether `pi` remains as a legacy alias to the same entrypoint.
- `bin/pi.mjs` (modified) — Refactor the entrypoint into an Omegon-owned executable contract, potentially split or renamed so the canonical surface is `bin/omegon.mjs` while legacy aliases re-enter the same runtime.
- `extensions/bootstrap/index.ts` (modified) — Change update verification helpers and operator messaging so `/update` proves and instructs relaunch through `omegon`, not `pi`. Preserve compatibility checks only as transitional guardrails.
- `scripts/install-pi.sh` (modified) — Replace or supersede with an Omegon-first install/relink script and update verification output to check the Omegon executable path.
- `README.md` (modified) — Rewrite install/start/update examples so the happy path uses `omegon` exclusively; mention `pi` only as a legacy compatibility alias if it remains.
- `docs/omegon-install.md` (modified) — Update distribution and lifecycle docs to describe the authoritative Omegon executable boundary and migration semantics for any legacy `pi` alias.
- `tests/bin-where.test.ts` (modified) — Extend executable probe tests to cover the canonical Omegon entrypoint and any renamed `--where`/ownership verification surface.
- `extensions/bootstrap/index.test.ts` (modified) — Update verification helper tests to validate Omegon-first path ownership and migration behavior for any `pi` compatibility alias.

## Constraints

- Direct operator instructions must use `omegon`, not `pi`, across install, update, and restart surfaces.
- If a `pi` alias remains, it must resolve to the same Omegon-owned entrypoint and must not bypass startup/update verification logic.
- The existing runtime ownership guarantees from singular-package-update-lifecycle must be preserved: dev mode and installed mode still verify the active runtime resolves to Omegon before restart handoff.
- Migration should avoid stranding existing installs; compatibility behavior for users who still type `pi` must be explicit and safe rather than accidental.
