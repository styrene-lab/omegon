# Vendor-neutral inference runtime model — Design

## Model

### Adapter

`AdapterId` names request/response semantics understood by Omegon. It replaces the endpoint's direct `InferenceInterface` field while retaining the current interface strings as initial adapter IDs. Adapter support remains compiled code; configuration selects an adapter but cannot inject executable behavior.

### Transport

`TransportSpec` is independent of adapter semantics:

- `Http { base_url }`
- `LocalProcess { command_ref }`
- `UnixSocket { path }`

Only non-secret connection coordinates are stored. Authentication remains a list of secret references. Each transport validates its own required fields.

### Endpoint

`InferenceEndpoint` is callable and has:

- adapter;
- transport;
- secret references;
- enabled state;
- optional `EndpointGroupId`;
- policy attributes;
- namespaced extension metadata.

No parent record is required. `EndpointGroup` is optional metadata for ownership and policy grouping, not a vendor control plane.

### Offering

Offerings continue to reference exactly one endpoint and retain modalities, capabilities, grades, context limits, optional conceptual identity, and opaque extension metadata.

### Policy attributes

Use a string-key/string-set map so locality, operator, trust, cost, and future dimensions compose without a closed execution-class enum. Core compatibility in this slice does not infer behavior from these values.

### Extension metadata

Use a string map with keys required to contain a namespace separator (`/`). Values are opaque strings. Snapshot merge preserves field provenance for the complete map. Core compatibility ignores the map.

## Migration

The module is not yet consumed by live execution, so this is the correct point for the breaking domain rename. The embedded registry adapter maps each existing provider endpoint to:

- an optional group with its current provider identity;
- an endpoint with adapter derived from `EndpointProtocol`;
- HTTP transport when a base URL exists, otherwise a local-process placeholder for Ollama and a provider-managed HTTP marker for compiled clients.

To avoid inventing executable coordinates, transport supports `Managed`, meaning connection construction remains owned by an existing compiled provider client. Runtime manifests introduced later will not be allowed to select `Managed`; it is a bootstrap compatibility representation only.

## Validation

Snapshot validation checks:

- non-empty IDs;
- optional group references;
- offering endpoint references;
- conceptual-model references;
- adapter IDs;
- transport-specific required fields;
- extension metadata namespaces;
- secret references are names, not apparent values.

## Security

- No executable command strings: local process uses a symbolic `command_ref` resolved by a compiled adapter or future allowlisted substrate.
- No secret values in inventory.
- Extension metadata is never executed or consulted for route eligibility.
- URLs are parsed only when execution wiring is added; this slice performs structural non-empty validation and leaves network policy enforcement to the execution boundary.
