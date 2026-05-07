+++
id = "959bf61a-e6a0-42ea-b136-d026a160e471"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Unified auth surface — single /login command, agent-callable, all backends — Tasks

## 1. core/crates/omegon/src/features/auth.rs (new)

- [x] 1.1 AuthFeature: auth_status tool (read-only probe of all backends) + provide_context for auth state injection + on_event for expiry notifications

## 2. core/crates/omegon/src/auth.rs (modified)

- [x] 2.1 Add probe_all_providers() → AuthStatus with per-backend detail. auth_status_to_provider_statuses() for HarnessStatus compat.

## 3. core/crates/omegon-secrets/src/resolve.rs (modified)

- [x] 3.1 Add vault recipe type: vault:secret/data/path#key → Vault KV v2 lookup. Fail-closed. Path traversal + null byte rejection.

## 4. core/crates/omegon/src/plugins/mcp.rs (modified)

- [x] 4.1 Add url field to McpServerConfig, connect via rmcp StreamableHttpTransport for HTTP servers, OAuth via rmcp auth feature. URL scheme validation.

## 5. core/crates/omegon/Cargo.toml (modified)

- [x] 5.1 Enable rmcp features: transport-streamable-http-client-reqwest, auth

## 6. core/crates/omegon/src/main.rs (modified)

- [x] 6.1 Replace Login subcommand with Auth { action: AuthAction } subcommand (status/login/logout/unlock), keep login as alias

## 7. core/crates/omegon/src/tui/mod.rs (modified)

- [x] 7.1 Add /auth slash command: no args → status table, /auth login <provider> → OAuth flow

## 8. core/crates/omegon/src/setup.rs (modified)

- [x] 8.1 Populate HarnessStatus.providers from probe_all_providers() via auth_status_to_provider_statuses()

## 9. core/crates/omegon-secrets/src/recipes.rs (modified)

- [x] 9.1 Add RecipeType::Vault { path, key } for Vault KV resolution

## 10. Cross-cutting constraints

- [x] 10.1 auth_status tool is read-only — agent can probe but never trigger login flows
- [x] 10.2 Vault recipe resolution fails closed: if Vault unreachable, recipe returns None
- [x] 10.3 MCP HTTP transport validates URL scheme (https only, or http://localhost for dev)
- [x] 10.4 OAuth tokens from Vault not logged or included in HarnessStatus events
- [x] 10.5 /auth login triggers browser-open for OAuth providers — requires operator presence
