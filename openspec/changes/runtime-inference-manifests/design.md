# Runtime inference manifest loading — Design

## Files and precedence

Default paths:

1. Organization: explicitly supplied only in this slice.
2. User: `$OMEGON_HOME/inference.toml`, falling back to `~/.omegon/inference.toml`.
3. Project: `<project-root>/.omegon/inference.toml`.
4. Session: explicitly supplied only.

`ManifestSource { source, path, required }` makes optional-vs-explicit behavior unambiguous. Callers construct the source list; path discovery has a pure helper for tests.

## Schema

TOML uses `schema_version = 1` and arrays of records:

```toml
schema_version = 1

[[endpoints]]
id = "private-chat"
adapter = "chat-completions"
enabled = true
secret_refs = ["PRIVATE_CHAT_TOKEN"]

[endpoints.transport]
kind = "http"
base_url = "https://inference.internal/v1"

[endpoints.policy]
network = ["private"]

[[offerings]]
id = "private-chat:model-a"
endpoint = "private-chat"
native_model_id = "model-a"
input_modalities = ["text"]
output_modalities = ["text"]
```

Patch records omit fields to preserve lower-layer values. New records are checked by existing snapshot construction for required fields.

Transport is a tagged TOML object. `managed` is rejected by conversion for every runtime source.

## Loader

`InferenceManifestLoader` owns an embedded bootstrap layer and a list of manifest sources. `load_layers` reads and parses all sources off-store. `reload(store)` calls `store.refresh` only after all parsing succeeds. Existing store validation and serialized generation activation provide atomic last-known-good behavior.

## Diagnostics

`ManifestDiagnostic` contains:

- scope/source;
- path;
- phase: read, parse, schema, conversion, validation;
- redacted message.

TOML parser messages can contain source snippets, so parse diagnostics expose line/column and parser classification only, not the raw parser display or file content. Validation errors already omit secret values.

## Adversarial review

The implementation review tightened four boundaries:

1. `managed` is rejected during manifest conversion as well as by snapshot validation, so runtime files cannot acquire bootstrap-only transport authority.
2. Diagnostics retain source scope and path but never include source file content or TOML parser excerpts that could contain credentials.
3. Missing optional sources are silent while missing required sources are explicit diagnostics; a required-source failure prevents activation.
4. Reload constructs every candidate layer before the inventory store is touched, preserving the complete previous snapshot on read, parse, conversion, or validation failure.

## Testing

Use temporary directories generated from process ID plus atomic counter; no new dependency. Tests cover valid standalone endpoints, precedence/provenance, optional missing files, required missing files, unsupported schema, malformed TOML redaction, managed transport rejection, token-like secret references, and store retention.

Focused evidence: `cargo test -p omegon inference_manifest --locked` passes 5 tests. Full repository tests, lint, and link remain required before completion.
