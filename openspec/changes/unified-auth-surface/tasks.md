# Unified auth surface — single /login command, agent-callable, all backends — Tasks

## 1. core/crates/omegon/src/features/auth.rs (new)

- [ ] 1.1 AuthFeature: auth_status tool (read-only probe of all backends) + provide_context for auth state injection + on_event for expiry notifications

## 2. core/crates/omegon/src/auth.rs (modified)

- [ ] 2.1 Add probe_all_providers() that returns Vec<ProviderStatus> with auth state for each backend. Unify credential check logic.

## 3. core/crates/omegon-secrets/src/resolve.rs (modified)

- [ ] 3.1 Add vault recipe type: resolve secret from Vault KV path when Vault is configured and unsealed

## 4. core/crates/omegon/src/plugins/mcp.rs (modified)

- [ ] 4.1 Add url field to McpServerConfig, connect via rmcp StreamableHttpTransport for HTTP servers, OAuth via rmcp auth feature

## 5. core/crates/omegon/Cargo.toml (modified)

- [ ] 5.1 Enable rmcp features: transport-streamable-http-client-reqwest, auth

## 6. core/crates/omegon/src/main.rs (modified)

- [ ] 6.1 Replace Login subcommand with Auth { action: AuthAction } subcommand (status/login/logout/unlock), keep login as alias

## 7. core/crates/omegon/src/tui/mod.rs (modified)

- [ ] 7.1 Add /auth slash command dispatching to auth.rs probe + login flows

## 8. core/crates/omegon/src/setup.rs (modified)

- [ ] 8.1 Populate HarnessStatus.providers from auth::probe_all_providers() at startup

## 9. core/crates/omegon-secrets/src/recipes.rs (modified)

- [ ] 9.1 Add RecipeType::Vault with path field for Vault KV resolution

## 10. Cross-cutting constraints

- [ ] 10.1 auth_status tool is read-only — agent can probe but never trigger login flows
- [ ] 10.2 Vault recipe resolution fails closed: if Vault unreachable, recipe returns None (never falls through to stale cache)
- [ ] 10.3 MCP HTTP transport must validate server URL scheme (https only, or http://localhost for dev)
- [ ] 10.4 OAuth tokens from Vault must not be logged or included in HarnessStatus events
- [ ] 10.5 /auth login triggers browser-open for OAuth providers — requires operator presence
