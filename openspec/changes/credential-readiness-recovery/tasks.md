## 1. Credential usability and refresh outcomes
<!-- specs: credentials -->

- [x] Add failing local-fixture tests for rejected expired credentials, precedence, and sanitized typed failure classification.
- [x] Implement bounded refresh adapters and shared sync/async usability rules.
- [x] Remove cached OAuth fallback after failed credential resolution.

## 2. Refresh coordination and explicit recovery
<!-- specs: credentials -->

- [x] Add request-count tests for concurrent refresh, terminal suppression, credential generation changes, and explicit retry.
- [x] Implement per-provider refresh coordination and transient retry interval.
- [x] Wire explicit connection retry to reset terminal suppression.

## 3. Discovery and verification
<!-- specs: credentials -->

- [x] Verify ordinary provider inventory performs no refresh requests.
- [x] Make provider status discovery read-only and preserve fresh external adoption at execution boundaries.
- [x] Run focused credential tests and OpenSpec validation; record results separately.
- [x] Complete landing crate checks and lint gates (root agent).
