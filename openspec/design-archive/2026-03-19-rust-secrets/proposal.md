+++
id = "edaaefa9-75db-4488-8d39-4a6272b3e9f1"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Port secrets system to Rust (redaction, recipes, tool guards)

## Intent

Port the 00-secrets extension: secret recipes (env, keychain, shell cmd), output redaction, tool guards for sensitive paths, audit log. Security-critical — must be in-process, not external.

See [design doc](../../../docs/rust-secrets.md).
