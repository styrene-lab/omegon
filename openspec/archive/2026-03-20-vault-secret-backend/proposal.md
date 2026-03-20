# Vault as first-class secret backend — operator-controlled secret storage with unseal lifecycle

## Intent

Elevate HashiCorp Vault from an incidental external tool to a first-class secret backend in the omegon-secrets crate. The operator should be able to choose to store a secret in Vault (not just env/keyring/file). The harness should be able to prompt the operator for unseal keys when Vault is sealed, manage the Vault lifecycle from the TUI, and resolve secrets from Vault paths as a recipe kind.
