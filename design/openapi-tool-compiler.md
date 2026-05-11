+++
id = "bf7ed939-a277-4635-9669-29f4da154de3"
kind = "design_node"
title = "OpenAPI tool compiler — any REST API becomes an agent tool from a spec file"
status = "decided"
tags = ["tools", "openapi", "integration", "extension", "api"]
aliases = ["openapi-tool-compiler"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = ["standalone crate or inline in omegon?", "auth credential injection flow"]
parent = "omega"
priority = "1"
related = ["extension-crate-migration"]
+++

# OpenAPI tool compiler — any REST API becomes an agent tool from a spec file

## Problem

Connecting omegon to a new SaaS API today requires writing a custom extension — Rust binary, manifest.toml, MCP or native protocol, build, install. That's a 200+ line investment per API. Meanwhile, most SaaS APIs publish an OpenAPI 3.x spec. A deterministic compiler that reads the spec and emits tool definitions would collapse that to a single config line.

## Prior art

Delfhos (Python agent SDK) implements this as `APITool` — a ~900-line OpenAPI compiler that resolves `$ref`, extracts path/query/body parameters, compresses endpoint descriptions for the LLM context window, caches compiled specs, and injects credentials per-request. The approach is proven: any API with an OpenAPI spec becomes callable in one line.

## Design

### Core abstraction

```rust
pub struct OpenApiToolProvider {
    spec: CompiledSpec,
    client: reqwest::Client,
    auth: AuthConfig,
}

pub struct CompiledSpec {
    base_url: String,
    endpoints: Vec<CompiledEndpoint>,
    compressed_docs: String,
}

pub struct CompiledEndpoint {
    operation_id: String,
    method: http::Method,
    path_template: String,
    parameters: Vec<Parameter>,
    request_body: Option<JsonSchema>,
    response_schema: Option<JsonSchema>,
    description: String,
}
```

### Compilation pipeline

1. **Parse** — read OpenAPI 3.0/3.1 YAML or JSON (use `serde_yaml` + `serde_json`)
2. **Resolve $ref** — inline all `$ref` pointers into concrete schemas (recursive, cycle-detected)
3. **Extract endpoints** — for each `paths.{path}.{method}`, produce a `CompiledEndpoint` with parameter schemas converted to JSON Schema for the tool definition
4. **Compress descriptions** — strip examples, reduce verbose descriptions to single lines, cap total docs size for context window efficiency
5. **Cache** — write compiled spec to `~/.config/omegon/api_cache/{hash}.json` keyed on spec content hash

### Tool registration

Each endpoint becomes a tool named `{spec_prefix}_{operation_id}` (e.g., `stripe_create_customer`). The tool's JSON schema is derived directly from the endpoint's parameters + request body.

Alternatively, register a single tool `api_call` that takes `operation_id` + params — fewer tools in the registry, but requires the LLM to know operation IDs.

**Decision:** Single tool per endpoint. LLMs perform better with explicit tool names than with a dispatch parameter.

### Authentication

```toml
# .omegon/settings.toml or per-spec config
[openapi.stripe]
spec = "https://raw.githubusercontent.com/stripe/openapi/master/openapi/spec3.json"
auth = "bearer"
secret = "STRIPE_API_KEY"   # resolved from omegon-secrets vault

[openapi.github]
spec = "specs/github-v3.yaml"
auth = "bearer"
secret = "GITHUB_TOKEN"
base_url_override = "https://api.github.com"
```

Auth modes: `bearer`, `api_key` (header or query), `basic`, `oauth2` (with token refresh via omegon's auth system).

### Execution

When the LLM calls a compiled tool:
1. Resolve the endpoint from operation_id
2. Template path parameters into the URL
3. Serialize query parameters
4. Serialize request body as JSON
5. Inject auth header from secret vault
6. Execute HTTP request via shared `reqwest::Client`
7. Return response body (JSON) as tool result, or structured error

### Filtering

Specs can be large (Stripe has 300+ endpoints). Support:
- `allow = ["customers.*", "charges.create"]` — glob patterns on operation_id
- `confirm = ["*.delete", "*.update"]` — require human approval for destructive operations
- `read_only = true` — only GET endpoints

## Scope

### Phase 1: Core compiler + static spec loading
- Parse OpenAPI 3.0 YAML/JSON
- $ref resolution
- Endpoint → tool definition compilation
- Bearer auth injection
- Cache compiled specs
- Register as core tool provider (not an extension — too foundational)

### Phase 2: Runtime spec discovery
- URL-based spec fetching with ETag caching
- `.omegon/apis/` directory for local spec overrides
- Per-endpoint allow/confirm/read_only filtering

### Phase 3: OAuth2 flows
- Token refresh integration with omegon's auth system
- OAuth2 authorization_code + client_credentials flows

## Critical files

| File | Purpose |
|---|---|
| `src/tools/openapi.rs` | Compiler + tool provider (~500 lines) |
| `src/tools/openapi_resolve.rs` | $ref resolver (~150 lines) |
| `src/tools/mod.rs` | Register OpenApiToolProvider |
| `src/tool_registry.rs` | Dynamic tool registration from compiled specs |

## Dependencies

- `serde_yaml` — parse OpenAPI YAML (already in dep tree via other paths)
- No new heavy deps. `reqwest` already available.
