+++
id = "6ecb049d-deb2-4437-9f71-ff69a835cb35"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Omegon binary identity — eliminate direct product exposure as `pi` — Tasks

## 1. Canonical executable boundary <!-- specs: runtime/binary-identity -->

- [x] 1.1 Promote `omegon` to the canonical package bin in `package.json`.
- [x] 1.2 Add a dedicated `bin/omegon.mjs` entrypoint that owns runtime resolution and `--where` metadata.
- [x] 1.3 Keep `bin/pi.mjs` only as a compatibility shim that immediately re-enters the Omegon entrypoint.

## 2. Update and verification flow <!-- specs: runtime/binary-identity -->

- [x] 2.1 Refactor `extensions/bootstrap/index.ts` verification helpers around the active `omegon` executable path.
- [x] 2.2 Rewrite `/update` completion and failure messaging so restart handoff says `omegon`, not `pi`.
- [x] 2.3 Preserve the singular-package verification contract in both dev and installed modes.

## 3. Contributor lifecycle script <!-- specs: runtime/binary-identity -->

- [x] 3.1 Update `scripts/install-pi.sh` so it verifies the `omegon` executable as the authoritative boundary.
- [x] 3.2 Keep any `pi` output explicitly compatibility-only rather than the primary success criterion.

## 4. Documentation surfaces <!-- specs: runtime/binary-identity -->

- [x] 4.1 Rewrite README install/start/update guidance to use `omegon` as the happy path.
- [x] 4.2 Update `docs/omegon-install.md` to document the canonical Omegon entrypoint and any legacy `pi` alias semantics.

## 5. Regression coverage <!-- specs: runtime/binary-identity -->

- [x] 5.1 Extend `tests/bin-where.test.ts` to cover `omegon --where` and the legacy `pi` shim path.
- [x] 5.2 Update `extensions/bootstrap/index.test.ts` to validate Omegon-first ownership checks and compatibility alias behavior.
