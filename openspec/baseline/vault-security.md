+++
id = "753311c2-fe64-4eea-a408-5d4469f0bfcf"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# vault-security

### Requirement: Client-side path allowlist enforcement (CHANGED — fail-closed)

Every Vault API call is checked against the configured allowlist before the HTTP request. An empty or missing allowlist denies all paths.

#### Scenario: Empty allowlist denies all paths

Given a VaultClient with allowed_paths []
And a valid token
When read("secret/data/omegon/api-keys") is called
Then no HTTP request is made to Vault
And an error is returned containing "no paths allowed"

#### Scenario: Missing allowlist denies all paths

Given a VaultConfig loaded from vault.json with no allowed_paths key
And a valid token
When read("secret/data/anything") is called
Then no HTTP request is made to Vault
And an error is returned containing "no paths allowed"

#### Scenario: Explicit allowlist permits matching paths

Given a VaultClient with allowed_paths ["secret/data/omegon/*"]
And a valid token
When read("secret/data/omegon/api-keys") is called
Then the Vault HTTP API is called
And the secret data is returned

#### Scenario: Path traversal rejected before glob check

Given a VaultClient with allowed_paths ["secret/data/*"]
And a valid token
When read("secret/data/../../sys/seal-status") is called
Then no HTTP request is made
And an error is returned containing "path traversal"

#### Scenario: URL-encoded traversal rejected

Given a VaultClient with allowed_paths ["secret/data/*"]
When read("secret/data/%2e%2e/sys/seal-status") is called
Then an error is returned containing "invalid characters"

### Requirement: VAULT_ADDR-only configuration starts deny-all

When only VAULT_ADDR is set (no vault.json), path policy defaults to DenyAll.

#### Scenario: VAULT_ADDR without vault.json starts deny-all

Given VAULT_ADDR is set to "http://vault:8200"
And no vault.json exists
When VaultConfig is loaded
Then allowed_paths is empty
And a warning is logged mentioning "create vault.json with allowed_paths"

#### Scenario: Health checks work despite deny-all path policy

Given a VaultClient with deny-all path policy
When health() is called
Then the health check succeeds
And no path enforcement error occurs

### Requirement: Auth failure produces no client

Failed authentication must not leave a half-initialized client in the SecretsManager.

#### Scenario: Auth failure results in no vault client

Given vault.json exists with a valid addr
And authentication fails (no token, no AppRole, no K8s)
When init_vault completes
Then vault_client is None
And a warning is logged
And vault: recipes return None

#### Scenario: Successful auth stores authenticated client

Given vault.json exists with a valid addr
And VAULT_TOKEN is set and valid
When init_vault completes
Then vault_client is Some
And the client has a valid token

### Requirement: Network security hardening

The HTTP client is configured defensively against token exfiltration and SSRF.

#### Scenario: Redirects are not followed

Given a Vault server that returns 301 to an attacker URL
When any authenticated request is made
Then the redirect is NOT followed
And X-Vault-Token is NOT sent to the redirect target

#### Scenario: Non-HTTP schemes rejected

Given a VaultConfig with addr "file:///etc/passwd"
When VaultClient::new is called
Then an error is returned containing "unsupported vault URL scheme"

#### Scenario: Connect timeout is shorter than total timeout

Given a VaultClient with default timeout_secs 30
When the client is created
Then connect_timeout is set to 10 seconds
And total timeout is set to 30 seconds

### Requirement: Error message sanitization

Vault API error responses must not propagate raw server response bodies.

#### Scenario: Read failure does not expose raw response body

Given an authenticated VaultClient
And the Vault server returns a 500 with body containing internal paths
When read("secret/data/omegon/test") is called
Then the error message contains the HTTP status code
And the error message does NOT contain the raw response body
