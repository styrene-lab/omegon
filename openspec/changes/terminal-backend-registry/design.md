# Design

See `docs/design/terminal-backend-registry.md`.

## Backend selection sketch

```text
terminal.create@1
  -> deserialize/schema validation
  -> origin attachment
  -> manifest policy
  -> runtime/operator policy
  -> TerminalBackendRegistry::select(params)
  -> backend.create(params)
  -> TerminalCreateResult
  -> audit/outcome
```

The registry owns placement semantics. Backends declare the placements they can satisfy. The built-in fallback declares `background_session` only. A future Flynt backend can declare `side_pane` or `bottom_pane`.
