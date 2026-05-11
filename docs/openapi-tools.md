+++
id = "0d2a5d67-388a-4f9f-a3b7-9f2e66734f8c"
kind = "document"
tags = ["tools", "openapi", "integrations"]
aliases = ["openapi-tool-provider"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
last_updated = "2026-05-11"
subsystem = "tool-provider"
+++

# OpenAPI Tool Provider

Omegon can compile project-local OpenAPI specs into structured agent tools at session startup. This is for REST APIs that should feel native to the agent without writing an extension or MCP server.

## Configuration

Create `.omegon/openapi.toml` at the project root. Each top-level table defines one API:

```toml
[linear]
spec = "api/linear.openapi.yaml"
auth = "bearer"
secret = "LINEAR_API_KEY"
read_only = true

[internal]
spec = "https://example.com/openapi.json"
auth = "X-API-Key"
secret = "INTERNAL_API_KEY"
base_url_override = "https://staging.example.com"
```

Fields:

| Field | Behavior |
| --- | --- |
| `spec` | Local JSON/YAML path, resolved relative to the project root, or an `http(s)` URL fetched at startup. |
| `auth` | `bearer`, `basic`, or a custom header name. |
| `secret` | Environment variable that contains the credential value. |
| `base_url_override` | Optional server URL override. If omitted, the first OpenAPI `servers[0].url` is used. |
| `read_only` | When `true`, only `GET` operations become tools. |

The config shape currently accepts `allow` and `confirm` arrays, but the provider does not enforce them yet. Do not rely on those fields for safety policy.

## Runtime Behavior

- Specs are loaded during agent setup from `.omegon/openapi.toml`.
- Tools are registered under the `openapi-tools` provider group.
- Tool names use the form `api_<config-name>_<operationId>`, normalized to snake case and truncated at 64 characters.
- When an operation has no `operationId`, the name is derived from the HTTP method and path.
- Path parameters, query parameters, and JSON request-body properties become tool parameters.
- `GET` operations are classified as repository-inspection capability; other methods are state-changing.
- HTTP responses are returned as text and truncated at 50 KB.
- Compile or fetch failures are logged and the OpenAPI tool provider is skipped for that session.

## Safety Notes

The provider is powerful because it lets the agent call arbitrary REST APIs. Use `read_only = true` for APIs where the agent should inspect but not mutate state, and scope API keys to the minimum permissions needed. Credentials are read from environment variables today; do not put raw secrets in `.omegon/openapi.toml`.
