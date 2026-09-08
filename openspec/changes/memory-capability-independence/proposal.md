# Independent memory capabilities

## Intent

An unavailable embedding service currently prevents setup from enabling session
extraction. Separate capability readiness so optional retrieval signals cannot
disable unrelated memory formation.

## Scope

Independent storage, extraction, embedding, and consolidation configuration and
readiness; bounded failures; typed status; recoverable indexing. Preserve existing
provider routing ownership. Resolve legacy default-model requirements explicitly
before implementation rather than silently selecting a replacement.

## Success criteria

- Configured extraction runs when embeddings are unavailable.
- Writes and lexical recall work when extraction is unavailable.
- Status distinguishes component availability and pending work.
- Capability failure does not fail an otherwise usable session.
