+++
id = "125b16e7-fbef-492a-827a-27354e5e2786"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Styrene Identity as operator credential root — RNS identity for secret unlocking and trust

## Intent

Should the operator's Styrene Identity (Ed25519 keypair from RNS, unique to their mesh node) serve as the root credential for unlocking Omegon's secret store? Today omegon-secrets uses keyring (OS credential store), Vault, env vars, and shell commands for secret resolution. A Styrene identity would add a cryptographic identity layer — the operator's mesh identity IS their Omegon identity, and possessing the RNS private key unlocks the secret store.

See [design doc](../../../docs/styrene-identity-secrets.md).
