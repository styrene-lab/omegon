+++
id = "5c9a03ed-99d1-47ad-80f9-09e3b58a073e"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Omegon Extension Authoring Reference

Task-scoped reference for the extension-authoring skill. Examples apply only to
authorized development work; check the current SDK compatibility contract first.

## Quick Start

```bash
omegon extension init my-extension
cd my-extension
cargo build --release
omegon extension install .
```

This scaffolds a working extension with manifest.toml, Cargo.toml, and src/main.rs.

## Extension Trait (Rust)

Extensions implement `omegon_extension::Extension`:

```rust
use omegon_extension::{Extension, serve, Error};
use serde_json::{json, Value};

#[async_trait::async_trait]
impl Extension for MyExt {
    fn name(&self) -> &str { "my-ext" }
    fn version(&self) -> &str { env!("CARGO_PKG_VERSION") }

    async fn handle_rpc(&self, method: &str, params: Value)
        -> omegon_extension::Result<Value>
    {
        match method {
            "get_tools" => Ok(json!([/* ToolDefinition array */])),
            "execute_tool" => {
                let tool_name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let args = params.get("args").cloned().unwrap_or_default();
                match tool_name {
                    "hello" => Ok(json!({"content": [{"type": "text", "text": args}]})),
                    _ => Err(Error::method_not_found(tool_name)),
                }
            }
            _ => Err(Error::method_not_found(method)),
        }
    }
}

#[tokio::main]
async fn main() { serve(MyExt::default()).await.unwrap(); }
```

## RPC Contract

Omegon calls these methods via JSON-RPC 2.0 over stdin/stdout:

| Method | When | Params | Returns |
|--------|------|--------|---------|
| `get_tools` | Startup handshake | `{}` | `[{name, label, description, parameters}]` |
| `bootstrap_secrets` | After get_tools | `{"SECRET_NAME": "value"}` | `{}` (ack) |
| `execute_tool` | Agent calls tool | `{name: "tool_name", args: {...}}` | `{content: [{type: "text", text: "..."}]}` |
| `get_<widget_id>` | TUI renders widget | `{}` | Widget-specific data |

All tools use the single `execute_tool` method. Dispatch on `params.name`; tool arguments are in `params.args`.

## manifest.toml Schema

```toml
[extension]
name = "my-ext"           # Required: lowercase alphanumeric + hyphens
version = "0.1.0"         # Required: semver
description = "..."       # Optional

[runtime]
type = "native"           # "native" or "oci"
binary = "target/release/my-ext"  # Relative path to compiled binary

[startup]
ping_method = "get_tools" # Readiness method (default)
timeout_ms = 5000         # Absolute readiness deadline (default)

[secrets]
required = ["API_KEY"]    # Must be in omegon vault before spawn
optional = ["DEBUG_KEY"]  # Extension degrades gracefully without

[widgets.dashboard]
label = "Dashboard"
kind = "stateful"         # "stateful" (tab) or "ephemeral" (modal)
renderer = "table"        # table, timeline, tree, graph

[mind]
enabled = true            # Persistent knowledge across sessions
max_facts = 500
retention_days = 90
```

## Tool Definition Format

Each tool in the `get_tools` response:

```json
{
  "name": "search_docs",
  "label": "Search Docs",
  "description": "Search documentation by keyword",
  "parameters": {
    "type": "object",
    "properties": {
      "query": {"type": "string", "description": "Search query"},
      "limit": {"type": "number", "description": "Max results", "default": 5}
    },
    "required": ["query"]
  }
}
```

## Security Model

- Extension processes are spawned with a clean environment (no parent env leakage)
- Secrets are delivered via `bootstrap_secrets` RPC, never environment variables
- Native and current OCI extensions are trusted host-authority code, not sandboxed code
- Installation and enablement do not authorize execution; trust `extension:<manifest name>` through `permissions.trustedContributionCode`
- Omegon computes and binds each runtime permit to the admitted source snapshot digest; authors do not declare this digest
- The stable profile grant persists across updates, so review new bytes before reinstalling them
- Panics in extension code crash only the extension, not the harness
- Transport failures consume a generation-local restart budget with capped backoff, then enter terminal quarantine
- Unix native process groups can provide strict cleanup; OCI and unowned boundaries report best-effort cleanup

Process or container separation provides crash isolation, not verified confinement.
`--dangerously-bypass-permissions` does not bypass dynamic-contribution preflight or grant code trust.

## Development Workflow

```bash
# Scaffold
omegon extension init my-ext && cd my-ext

# Develop (local install copies into Omegon's guarded extension root)
cargo build --release
omegon extension install .

# Test
omegon                       # start TUI, extension loads automatically

# Iterate: rebuild, reinstall/update the copied bundle, then restart Omegon
cargo build --release
omegon extension remove my-ext
omegon extension install .

# Ship
omegon extension remove my-ext
# push to git, then:
omegon extension install https://github.com/user/my-ext
```

## Crate Reference

The `omegon-extension` Rust SDK is published separately from the Omegon host.
Its canonical source is <https://github.com/styrene-lab/omegon-extension-rs>;
consumers should follow that repository's current dependency and compatibility guidance.

```toml
[dependencies]
omegon-extension = "<current compatible version>"
```

Host-owned runtime behavior is documented in `docs/extensions.md`. SDK-owned
protocol types, compatibility versions, and authoring helpers live in the
standalone SDK repository and must not be inferred from host internals.
