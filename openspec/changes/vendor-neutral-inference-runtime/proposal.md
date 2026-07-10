# Vendor-neutral inference runtime model

## Intent

Refine the inference inventory foundation so Omegon's core execution graph models adapters, transports, endpoints, and offerings without requiring or encoding a vendor administrative control plane.

## Scope

- Replace mandatory provider-integration parentage with optional neutral endpoint grouping.
- Separate adapter identity from transport configuration.
- Represent policy attributes as independent, extensible fields.
- Preserve namespaced extension metadata without interpreting it in core routing.
- Keep the embedded `ModelRegistry` projection as a compatibility/bootstrap adapter.
- Add regression fixtures proving the model works for local, public, private, brokered, and opaque endpoints without vendor-specific core types.

## Non-goals

- Runtime manifest parsing and file watching.
- Vendor/platform discovery connectors.
- Migrating live provider execution to the inventory.
- Benchmark collection or grade synthesis.

## Success criteria

1. A callable endpoint does not require an administrative provider record.
2. Adapter and transport are independently represented and validated.
3. One endpoint can expose heterogeneous offerings without provider-wide capability inference.
4. Namespaced extension metadata round-trips but cannot influence core compatibility.
5. The embedded registry projects into the neutral model without changing current execution behavior.
