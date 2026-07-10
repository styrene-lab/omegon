# Runtime inference manifest loading and atomic reload

## Intent

Load vendor-neutral inference inventory layers from declarative TOML manifests and activate complete snapshots atomically while retaining the last known-good generation on read, parse, or validation failure.

## Scope

- Define a versioned TOML manifest schema for endpoint groups, conceptual models, endpoints, and offerings.
- Resolve organization, user, project, and session manifest paths in deterministic precedence order.
- Parse manifests into existing sparse `InventoryLayer` records without admitting bootstrap-only managed transports.
- Add reload diagnostics that identify source and phase without exposing secret values.
- Add a loader/reloader that composes embedded bootstrap inventory with runtime layers and activates through `InferenceInventoryStore`.
- Add conformance tests for valid layering, malformed input, invalid records, missing optional files, and last-known-good retention.

## Non-goals

- File-system watching or automatic debounce.
- Discovery/probe connectors.
- Live provider dispatch migration.
- Secret resolution.
- Benchmark ingestion.

## Success criteria

1. A project manifest can add a standalone HTTP or local endpoint without recompilation.
2. Higher-precedence manifests override only supplied fields and preserve provenance.
3. Missing optional manifests are ignored; unreadable configured manifests are diagnosed.
4. Parse or validation failure leaves the active snapshot unchanged.
5. Diagnostics contain path/source/phase but never credential values.
