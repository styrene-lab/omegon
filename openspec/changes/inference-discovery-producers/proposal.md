# Discovery-layer producers and model catalog unification

## Intent

Populate the dynamic inference inventory's Discovery layer with live provider model enumeration (protocol-keyed fetchers, not per-provider), run discovery as a TTL-cached non-blocking background refresh with last-known-good persistence, and migrate the operator-visible model catalog from the static embedded registry to inventory snapshot projection — so provider model churn (e.g. GitHub Copilot serving 29 live models while the registry lists 4) surfaces automatically instead of requiring hand-curated registry updates that scale with provider×model count. Target release: 0.28.2.

## Scope

_TBD_

## Constraints

_None identified yet._
