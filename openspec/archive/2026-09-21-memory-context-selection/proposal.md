# Shared task-aware memory selection

## Intent

Replace ambient list-and-render behavior with explicit selection. Current ambient
injection bypasses search confidence and relevance, packs Architecture first, and
can stop before a smaller relevant constraint is considered.

## Scope

A shared domain selector for ambient memory, explicit context packs, and standalone
provider context; token-aware packing; applicable pins; provenance handles; cached
selection invalidation; auditable selection outcomes. Rendering remains presentation.

## Success criteria

- Relevant constraints survive irrelevant Architecture floods.
- All memory context surfaces share eligibility and packing contracts.
- The emitted block fits its allocated token budget, including metadata.
- Rejected and selected evidence can be inspected without content logging.
