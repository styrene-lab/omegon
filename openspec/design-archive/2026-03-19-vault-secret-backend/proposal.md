+++
id = "02d4c3a9-1d97-419f-865f-580133e918e0"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Vault as first-class secret backend — operator-controlled secret storage with unseal lifecycle

## Intent

Elevate HashiCorp Vault from an incidental external tool to a first-class secret backend in the omegon-secrets crate. The operator should be able to choose to store a secret in Vault (not just env/keyring/file). The harness should be able to prompt the operator for unseal keys when Vault is sealed, manage the Vault lifecycle from the TUI, and resolve secrets from Vault paths as a recipe kind.

See [design doc](../../../docs/vault-secret-backend.md).
