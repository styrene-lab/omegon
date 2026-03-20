# vault-client

### Requirement: Vault recipe resolution

The `vault:` recipe kind resolves secrets from Vault KV v2 via HTTP API.

#### Scenario: Resolve a secret from Vault

Given a VaultClient configured with addr "http://vault:8200" and a valid token
And a recipe "vault:secret/data/omegon/api-keys#anthropic" exists
And Vault has key "anthropic" with value "sk-ant-test123" at path "secret/data/omegon/api-keys"
When resolve_secret("ANTHROPIC_API_KEY") is called
Then the result is Some("sk-ant-test123")
And the resolved value is added to the redaction set

#### Scenario: Vault unreachable returns None

Given a VaultClient configured with addr "http://vault:8200"
And the Vault server is not reachable
And a recipe "vault:secret/data/omegon/keys#token" exists
When resolve_secret("MY_TOKEN") is called
Then the result is None
And a warning is logged
And no error is propagated to the caller

#### Scenario: Vault sealed returns None

Given a VaultClient configured with addr "http://vault:8200"
And the Vault server responds with sealed status
When resolve_secret with a vault: recipe is called
Then the result is None
And a warning is logged mentioning sealed state

### Requirement: Client-side path allowlist enforcement

Every Vault API call is checked against the configured allowlist before the HTTP request is made.

#### Scenario: Allowed path succeeds

Given a VaultClient with allowed_paths ["secret/data/omegon/*"]
And Vault is unsealed with a valid token
When read("secret/data/omegon/api-keys") is called
Then the Vault HTTP API is called
And the secret data is returned

#### Scenario: Disallowed path is rejected before HTTP call

Given a VaultClient with allowed_paths ["secret/data/omegon/*"]
When read("secret/data/bootstrap/cloudflare/vanderlyn") is called
Then no HTTP request is made to Vault
And an error is returned with message containing "path not in allowlist"

#### Scenario: Denied path overrides allowed

Given a VaultClient with allowed_paths ["secret/data/*"] and denied_paths ["secret/data/bootstrap/cloudflare/*"]
When read("secret/data/bootstrap/cloudflare/vanderlyn") is called
Then no HTTP request is made to Vault
And an error is returned with message containing "path denied"

### Requirement: Auth method negotiation

The client tries auth methods in priority order until one succeeds.

#### Scenario: VAULT_TOKEN env is used first

Given VAULT_TOKEN env is set to "hvs.testtoken"
And AppRole config exists in vault.json
When the VaultClient authenticates
Then the token "hvs.testtoken" is used
And AppRole is not attempted

#### Scenario: AppRole fallback when no token

Given VAULT_TOKEN env is not set
And no ~/.vault-token file exists
And vault.json has auth "approle" with role_id "test-role"
And secret_id "test-secret" is in the keyring
When the VaultClient authenticates
Then POST /v1/auth/approle/login is called with role_id and secret_id
And the returned client_token is cached

#### Scenario: K8s SA auth in-cluster

Given VAULT_TOKEN env is not set
And no AppRole config exists
And /var/run/secrets/kubernetes.io/serviceaccount/token exists
And vault.json has auth "kubernetes" with role "omegon-agent"
When the VaultClient authenticates
Then POST /v1/auth/kubernetes/login is called with the SA JWT and role
And the returned client_token is cached

### Requirement: Child token minting for cleave

Parent agents can mint scoped tokens for child tasks.

#### Scenario: Mint a child token with restricted policy

Given a VaultClient with a token that has auth/token/create capability
When mint_child_token(policies=["omegon-child"], ttl="30m", use_limit=100) is called
Then POST /v1/auth/token/create is called with the specified policies, ttl, and num_uses
And the returned token is returned to the caller

### Requirement: Unseal lifecycle via TUI

The operator can unseal Vault through the TUI without the agent seeing the keys.

#### Scenario: Unseal progress display

Given Vault is sealed with threshold 3 of 5
When the operator enters `/vault unseal` and provides one key
Then the TUI shows "Unseal progress: 1/3"
And the key is not echoed to the screen
And the key is not stored in command history

#### Scenario: Full unseal sequence

Given Vault is sealed with threshold 3 of 5
When the operator provides 3 valid unseal keys via `/vault unseal`
Then Vault becomes unsealed
And the TUI shows "Vault unsealed"
And a SystemNotification is emitted

### Requirement: Startup health check

Vault status is checked on startup and surfaced to the operator.

#### Scenario: Vault configured and healthy

Given vault.json exists with a valid addr
And Vault is unsealed and reachable
When omegon starts an interactive session
Then no notification is shown about Vault
And whoami includes "vault: active (addr)"

#### Scenario: Vault configured but sealed

Given vault.json exists with a valid addr
And Vault is reachable but sealed
When omegon starts an interactive session
Then a SystemNotification is shown: "Vault is sealed — secrets from Vault unavailable. Use /vault unseal"
And vault: recipes degrade to None

#### Scenario: Vault not configured

Given no vault.json exists and VAULT_ADDR is not set
When omegon starts an interactive session
Then no Vault check is performed
And whoami does not include a Vault section
