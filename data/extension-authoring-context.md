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
            "execute_<tool_name>" => { /* handle tool call */ }
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
| `execute_<tool_name>` | Agent calls tool | Tool args object | `{content: [{type: "text", text: "..."}]}` |
| `get_<widget_id>` | TUI renders widget | `{}` | Widget-specific data |

Tool names from `get_tools` are prefixed with `execute_` for dispatch: tool "hello" -> method "execute_hello".

## manifest.toml Schema

```toml
[extension]
name = "my-ext"           # Required: lowercase alphanumeric + hyphens
version = "0.1.0"         # Required: semver
description = "..."       # Optional
sdk_version = "0.15"      # Optional: prefix-matched at install

[runtime]
type = "native"           # "native" or "oci"
binary = "target/release/my-ext"  # Relative path to compiled binary

[startup]
ping_method = "get_tools" # Health check method (default)
timeout_ms = 5000         # Health check timeout (default)

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
- Extensions cannot access the agent's conversation, credentials, or filesystem outside their directory
- Panics in extension code crash only the extension, not the harness
- Extensions auto-disable after repeated crashes within a session

## Development Workflow

```bash
# Scaffold
omegon extension init my-ext && cd my-ext

# Develop (symlink mode — changes apply on restart)
cargo build --release
omegon extension install .   # creates symlink

# Test
omegon                       # start TUI, extension loads automatically

# Iterate
cargo build --release        # rebuild
# restart omegon to pick up changes

# Ship
omegon extension remove my-ext   # remove symlink
# push to git, then:
omegon extension install https://github.com/user/my-ext
```

## Crate Reference

The `omegon-extension` crate is at `core/crates/omegon-extension/`. Key files:
- `lib.rs` — `serve()` function, safety docs
- `extension.rs` — `Extension` trait definition
- `rpc.rs` — JSON-RPC message types
- `manifest.rs` — `ExtensionManifest` struct with validation
- `error.rs` — Error codes (MethodNotFound, InvalidParams, etc.)
