+++
id = "61da9829-6fc2-40b3-a84b-9cd10ac73853"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Omegon Extension SDK

The Omegon Extension SDK (`omegon-extension` crate) provides a safe, versioned interface for third-party developers to build extensions for Omegon.

**Core principle:** Extension failures must not crash Omegon. Safety is enforced at install time and runtime.

## Fastest Path

```bash
omegon extension init my-extension
cd my-extension
cargo build --release
omegon extension install .
```

This scaffolds a working v1 extension with one tool and a manifest. Edit `src/main.rs` to add your logic. Done.

## Protocol Options

| | v1 | v2 |
|---|---|---|
| **Direction** | Host → extension only | Bidirectional |
| **Entry point** | `serve(ext)` | `serve_v2(ext)` |
| **Use when** | You provide tools, widgets, or resources | You need to send progress notifications, request sampling, or read host state |
| **Complexity** | One trait, one function | Adds `HostProxy` + `on_initialized()` callback |
| **Start here?** | Yes | Migrate from v1 when you need it |

If you're building your first extension, use v1. You can switch to v2 later by adding `on_initialized()` and changing `serve()` to `serve_v2()`.

## Quick Start (Manual Setup)

If you prefer to set up the project yourself instead of using `omegon extension init`:

### 1. Create a new Rust project

```bash
cargo new my-omegon-extension
cd my-omegon-extension
```

### 2. Add the SDK to `Cargo.toml`

```toml
[dependencies]
omegon-extension = "0.19"
tokio = { version = "1", features = ["full"] }
serde_json = "1"
async-trait = "0.1"
```

Use the SDK version that matches the Omegon release you target. For source-checkout development, point the dependency at this repository instead of crates.io.

### 3. Implement the `Extension` trait

Create `src/main.rs`:

```rust
use omegon_extension::{Extension, Result};
use async_trait::async_trait;
use serde_json::{json, Value};

#[derive(Default)]
struct MyExtension;

#[async_trait]
impl Extension for MyExtension {
    fn name(&self) -> &str {
        "my-extension"
    }

    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    async fn handle_rpc(&self, method: &str, params: Value) -> Result<Value> {
        match method {
            "get_tools" => Ok(json!([
                {
                    "name": "my_tool",
                    "description": "My custom tool",
                    "input_schema": { "type": "object", "properties": {} }
                }
            ])),
            "get_timeline" => Ok(json!({
                "events": [
                    {
                        "title": "Example Event",
                        "timestamp": "2024-01-01T00:00:00Z",
                        "description": "An example timeline event"
                    }
                ]
            })),
            "execute_tool" => {
                let name = params["name"].as_str().unwrap_or("");
                let args = params.get("args").cloned().unwrap_or_default();
                match name {
                    "my_tool" => Ok(json!({
                        "content": [{
                            "type": "text",
                            "text": format!("called my_tool with {}", args)
                        }]
                    })),
                    _ => Err(omegon_extension::Error::method_not_found(name)),
                }
            }
            _ => Err(omegon_extension::Error::method_not_found(method)),
        }
    }
}

#[tokio::main]
async fn main() {
    let ext = MyExtension::default();
    omegon_extension::serve(ext)
        .await
        .expect("extension serve loop failed");
}
```

### 4. Create a manifest

Create `manifest.toml` in your extension directory:

```toml
[extension]
name = "my-extension"
version = "0.1.0"
description = "My custom extension"

[runtime]
type = "native"
binary = "target/release/my-extension"

[startup]
ping_method = "get_tools"
timeout_ms = 5000

[widgets.timeline]
label = "Timeline"
kind = "stateful"
renderer = "timeline"
description = "Activity timeline"
```

### 5. Build and install

```bash
# Build release binary
cargo build --release

# Install to ~/.omegon/extensions/my-extension/
mkdir -p ~/.omegon/extensions/my-extension
cp target/release/my-extension ~/.omegon/extensions/my-extension/
cp manifest.toml ~/.omegon/extensions/my-extension/

# Omegon will auto-discover on next startup
```

## Architecture

### Process Isolation

Extensions run as **separate processes**, not as libraries. This provides:

- **Crash isolation** — extension panics don't crash Omegon
- **Resource isolation** — each extension has its own memory, file descriptors
- **Version decoupling** — extensions can be updated independently
- **Language independence** — extensions can be written in any language

### RPC Protocol

Communication happens via **JSON-RPC 2.0 over stdin/stdout**:

```
Omegon (parent)  ←→  Extension (child process)
         ↓
    [stdin/stdout]
         ↓
   JSON-RPC 2.0
   Line-delimited
```

Each request and response is a single JSON object, newline-delimited.

**Request:**
```json
{"jsonrpc": "2.0", "id": "1", "method": "get_timeline", "params": {}}
```

**Response (success):**
```json
{"jsonrpc": "2.0", "id": "1", "result": {"events": [...]}}
```

**Response (error):**
```json
{"jsonrpc": "2.0", "id": "1", "error": {"code": "InternalError", "message": "..."}}
```

### Install-Time Safety Checks

When Omegon discovers an extension, it performs:

1. **Manifest parsing** — validates TOML structure
2. **Schema validation** — checks required fields (name, version, binary/image, widgets)
3. **Binary/image validation** — verifies native binary exists or OCI image is pullable
4. **Startup health check** — calls `ping_method` on startup, fails if unresponsive

**Extensions that fail any check are disabled before TUI starts** — no crashes at runtime.

### Runtime Safety

1. **Timeouts** — RPC calls have hard timeouts (configurable per extension)
2. **Error isolation** — method errors return error objects, not crashes
3. **Type validation** — serde validates all JSON on both sides
4. **Shutdown handling** — Omegon gracefully shuts down extensions on exit

## API Reference

### `Extension` Trait

```rust
#[async_trait]
pub trait Extension: Send + Sync {
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    async fn handle_rpc(&self, method: &str, params: Value) -> Result<Value>;
}
```

Implement this trait to handle RPC calls from Omegon.

### Error Codes

Extensions return typed errors via the SDK:

```rust
// Method not found
Error::method_not_found("unknown_method")

// Invalid parameters
Error::invalid_params("expected 'id' field")

// Internal server error (non-fatal)
Error::internal_error("database connection failed")

// Version mismatch (available for extension-side compatibility checks)
Error::version_mismatch("0.15", "0.16")

// Manifest error (caught at install time)
Error::manifest_error("missing required field: name")

// Timeout
Error::timeout()

// Not implemented
Error::not_implemented("streaming responses")
```

### RPC Methods

The SDK defines **standard methods** that Omegon expects:

#### `get_tools`

Return list of tools the extension provides.

**Request:**
```json
{"jsonrpc": "2.0", "id": "1", "method": "get_tools", "params": {}}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": "1",
  "result": [
    {
      "name": "analyze_code",
      "description": "Analyze Python code for errors",
      "input_schema": {
        "type": "object",
        "properties": {
          "code": {"type": "string"},
          "language": {"type": "string"}
        },
        "required": ["code"]
      }
    }
  ]
}
```

#### `get_<widget_id>`

Return initial data for a widget. Called on extension startup and when user opens the tab.

**Request:**
```json
{"jsonrpc": "2.0", "id": "1", "method": "get_timeline", "params": {}}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": "1",
  "result": {
    "events": [
      {
        "title": "Event 1",
        "timestamp": "2024-01-01T00:00:00Z",
        "description": "Description"
      }
    ]
  }
}
```

#### `execute_tool`

Execute a tool. Called by Omegon when the user invokes a tool.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": "2",
  "method": "execute_tool",
  "params": {
    "name": "analyze_code",
    "args": {
      "code": "print('hello')",
      "language": "python"
    }
  }
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": "2",
  "result": {
    "status": "ok",
    "errors": [],
    "warnings": []
  }
}
```

## Manifest Configuration

### Extension Metadata

```toml
[extension]
name = "my-extension"              # Must be lowercase, alphanumeric + hyphens
version = "0.1.0"                  # Semantic version
description = "Description..."     # Optional
```

### Runtime Configuration

**Native (local binary):**
```toml
[runtime]
type = "native"
binary = "target/release/my-extension"
```

The path is relative to the manifest directory.

**OCI Container:**
```toml
[runtime]
type = "oci"
image = "my-extension:latest"
```

Omegon will run via `podman run --rm -i my-extension:latest`.

### Startup Configuration

```toml
[startup]
ping_method = "get_tools"   # RPC method to call for health check
timeout_ms = 5000            # Timeout in milliseconds
```

If the health check fails, the extension is marked as unavailable.

### Widgets

Define UI tabs/modals:

```toml
[widgets.timeline]
label = "Timeline"           # Tab label in TUI
kind = "stateful"            # "stateful" (tab) or "ephemeral" (modal)
renderer = "timeline"        # "timeline", "tree", "table", "graph", etc.
description = "Activity..."  # Optional
```

For each widget `{id}`, the extension must implement `get_{id}` RPC method.

## Best Practices

### 1. Version Lock Your Dependencies

Lock the SDK version to the release line you target, or use a source dependency while developing against a checkout:

```toml
[dependencies]
omegon-extension = "0.19"
```

### 2. Validate Parameters Early

Return `Error::invalid_params()` for malformed requests:

```rust
async fn handle_rpc(&self, method: &str, params: Value) -> Result<Value> {
    match method {
        "analyze" => {
            let code = params.get("code")
                .and_then(|v| v.as_str())
                .ok_or_else(|| Error::invalid_params("expected 'code' string"))?;
            // ...
        }
        _ => Err(Error::method_not_found(method)),
    }
}
```

### 3. Never Block Forever

If `handle_rpc` blocks indefinitely, the entire extension hangs. Set your own timeouts:

```rust
async fn handle_rpc(&self, method: &str, params: Value) -> Result<Value> {
    match method {
        "slow_operation" => {
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(30),
                self.slow_operation(&params),
            )
            .await
            .map_err(|_| Error::timeout())?;
            Ok(result)
        }
        _ => Err(Error::method_not_found(method)),
    }
}
```

### 4. Graceful Error Handling

Return error objects, don't panic:

```rust
async fn handle_rpc(&self, method: &str, params: Value) -> Result<Value> {
    match method {
        "database_query" => {
            match self.query_db(&params).await {
                Ok(result) => Ok(result),
                Err(e) => Err(Error::internal_error(e.to_string())),
            }
        }
        _ => Err(Error::method_not_found(method)),
    }
}
```

### 5. Test Your Extension

The SDK includes test utilities:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_tools() {
        let ext = MyExtension::default();
        let result = ext.handle_rpc("get_tools", json!({})).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_unknown_method() {
        let ext = MyExtension::default();
        let result = ext.handle_rpc("unknown", json!({})).await;
        assert!(result.is_err());
    }
}
```

## Troubleshooting

### Extension not discovered

- Check manifest is at `~/.omegon/extensions/{name}/manifest.toml`
- Verify manifest TOML syntax: `toml-cli ~/.omegon/extensions/{name}/manifest.toml`
- Check that `extension.name` matches the directory name

### Extension fails health check

- Ensure the binary is built and at the path specified in `runtime.binary`
- Check that `startup.ping_method` is implemented and returns success
- Increase `startup.timeout_ms` if the extension needs more time to start

### RPC calls timeout

- Ensure `handle_rpc` doesn't block indefinitely
- Use `tokio::time::timeout()` for long operations
- Increase `startup.timeout_ms` in manifest if needed

### Type validation fails

- Ensure all JSON you return matches the expected schema
- Use `serde_json::json!()` macro to construct responses
- Validate incoming params with `.as_str()`, `.as_object()`, etc.

## Version Compatibility

The SDK crate version defines the Rust API contract between Omegon and extensions. The manifest schema currently does not require an `sdk_version` field; compatibility is managed through the extension protocol and the SDK dependency you build against.

**Breaking changes** (next major version):
- New required RPC methods
- Changed error codes
- Removed features

**Non-breaking changes** (minor version):
- New optional RPC methods
- New optional manifest fields
- New error codes (old code still works)

## Examples

### Published Extensions

These first-party extensions serve as real-world references:

- **[Vox](https://github.com/styrene-lab/vox)** — Communication connector (Discord, Slack, Signal, email). Demonstrates vox bridge polling, secrets management, and multi-connector architecture.
- **[Scry](https://github.com/styrene-lab/scry)** — Local image generation (FLUX, SDXL, SD1.5). Demonstrates widget declarations (Gallery, Preview, Models) and ComfyUI integration.
- **[scribe-rpc](https://github.com/styrene-lab/scribe-rpc)** — Minimal example using the Extension SDK.

## Support

For issues, questions, or feature requests:

- **GitHub Issues:** https://github.com/styrene-lab/omegon/issues
- **Documentation:** https://omegon.dev/extensions
- **Discord:** [Community server]

---

**Happy extending!** 🚀
