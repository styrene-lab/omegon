+++
id = "15c420f3-8777-4347-8c7b-f7c866a0eb35"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Omegon Extension Ecosystem

This document provides an overview of Omegon's extension system — how it works, what's available for developers, and how to build extensions.

## Build your first extension in 60 seconds

```bash
omegon extension init my-extension
cd my-extension
cargo build --release
omegon extension install .
```

That's it. You have a working extension with one tool. Edit `src/main.rs` to add your logic.

For the full SDK reference, see [EXTENSION_SDK.md](EXTENSION_SDK.md). For advanced patterns (widgets, state management, and bidirectional protocol work), see [EXTENSION_INTEGRATION.md](EXTENSION_INTEGRATION.md).

## Overview

Omegon's extension system allows third-party developers to add tools, widgets, and features without modifying the core codebase or coupling to its release cycle.

**Key design principles:**
- **Safety first**: Extension crashes don't crash Omegon
- **Version stability**: Extensions declare SDK version, validated at install time
- **Process isolation**: Extensions run as separate processes, communicate via RPC
- **Developer friendly**: Minimal boilerplate, clear API contract

## Architecture

### Components

```
Omegon (parent process)
    ↓
    ├─ Extension Discovery (startup)
    │   ├─ Scans ~/.omegon/extensions/
    │   ├─ Parses manifest.toml files
    │   └─ Validates metadata and SDK version
    │
    ├─ Extension Spawning
    │   ├─ Native: direct binary execution
    │   └─ OCI: podman run --rm -i image
    │
    ├─ RPC Communication
    │   └─ JSON-RPC 2.0 over stdin/stdout
    │
    ├─ Widget Rendering
    │   ├─ Tabs registered from widget declarations
    │   └─ Data fetched on demand via RPC
    │
    └─ Tool Integration
        ├─ Tools discovered from extensions
        ├─ Tools callable from conversation
        └─ Results rendered in conversation view
```

### Extension Lifecycle

1. **Installation** (manual)
   - Developer builds extension with `omegon-extension` SDK
   - Installs to `~/.omegon/extensions/{name}/`
   - Contains: binary/OCI image + manifest.toml

2. **Discovery** (TUI startup)
   - Omegon scans `~/.omegon/extensions/`
   - Parses manifest.toml for each extension
   - Validates schema (name, version, runtime type, widgets)

3. **Validation** (TUI startup)
   - Binary/image existence check
   - If validation fails, extension is disabled (logged)

4. **Spawning** (TUI startup)
   - Extension binary launched or OCI image pulled
   - stdin/stdout open for RPC communication

5. **Health Check** (TUI startup)
   - RPC call to `ping_method` (default: `get_tools`)
   - If timeout or error, extension is disabled
   - If success, extension is ready

6. **Registration** (TUI startup)
   - Widgets declared in manifest become tabs in UI
   - Tools declared become available in conversation
   - Extension marked as active

7. **Runtime** (TUI running)
   - User can invoke tools → Omegon calls `execute_tool` with `{ "name": "...", "args": {...} }`
   - User can open widget tabs → Omegon calls `get_{widget_id}` RPC
   - Extension responds with results
   - Omegon renders in UI

8. **Shutdown** (TUI exit)
   - Omegon sends SIGTERM to extension processes
   - Waits for graceful shutdown
   - Closes stdin/stdout

## Developer Documentation

### For Extension Developers

Start with **[EXTENSION_SDK.md](./EXTENSION_SDK.md)** — 5-minute quick start:

1. Create a Rust project with `omegon-extension` dependency
2. Implement the `Extension` trait
3. Create manifest.toml
4. Install to `~/.omegon/extensions/{name}/`

### Advanced Patterns

See **[EXTENSION_INTEGRATION.md](./EXTENSION_INTEGRATION.md)** for:

- Tool design patterns
- Widget patterns (timeline, memory, custom)
- State management
- Performance optimization
- Testing strategies
- Publishing to GitHub/OCI registries

### API Reference

The `omegon-extension` crate exports:

- **`Extension` trait**: Implement to handle RPC calls
- **`Error` enum**: Typed error codes with install-time flags
- **`RpcMessage`, `RpcRequest`, `RpcResponse`**: JSON-RPC types
- **`ExtensionManifest`**: Manifest validation

All types are fully documented with examples.

## Standard RPC Methods

Every extension should implement (or intentionally reject) these methods:

### `get_tools`

Return list of tools provided by the extension.

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
      "name": "my_tool",
      "description": "Tool description",
      "input_schema": {"type": "object", "properties": {...}}
    }
  ]
}
```

### `get_{widget_id}`

Return initial data for a widget.

**Request:**
```json
{"jsonrpc": "2.0", "id": "2", "method": "get_timeline", "params": {}}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": "2",
  "result": {
    "events": [
      {"title": "...", "timestamp": "...", "description": "..."}
    ]
  }
}
```

### `execute_tool`

Execute a tool with user-provided parameters.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": "3",
  "method": "execute_tool",
  "params": {
    "name": "my_tool",
    "args": {"arg1": "value"}
  }
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": "3",
  "result": {"status": "success", "output": "..."}
}
```

## Safety Model

### Install-Time Checks

Errors caught **before the extension ever runs**:

- Manifest TOML parse error
- Missing required manifest fields
- Invalid field values (name not lowercase, version not semver)
- SDK version mismatch
- Binary not found (native) or image not pullable (OCI)

These errors prevent installation. The extension won't run.

### Runtime Safety

Guarantees **during TUI operation**:

1. **Process isolation**: Extension crash → EOF detected, extension disabled, Omegon continues
2. **RPC timeouts**: Unresponsive extension → timeout error returned, Omegon continues
3. **Type validation**: Malformed JSON → serde error, extension returns error object
4. **Error propagation**: Extension error → typed error code in response, user sees error message
5. **Graceful shutdown**: SIGTERM → extension has 5 seconds to shut down, then SIGKILL

### Error Codes

Extensions return typed errors:

- `MethodNotFound`: Unknown RPC method
- `InvalidParams`: Malformed parameters
- `InternalError`: Extension error (non-fatal)
- `ManifestError`: Invalid manifest (fatal)
- `VersionMismatch`: SDK version incompatible (fatal)
- `Timeout`: RPC call timed out
- `ParseError`: Malformed JSON
- `NotImplemented`: Feature not implemented

## Examples

### Example 1: Simple Tool Extension

A Python code analyzer extension:

```rust
#[async_trait]
impl Extension for PythonAnalyzer {
    fn name(&self) -> &str { "python-analyzer" }
    fn version(&self) -> &str { env!("CARGO_PKG_VERSION") }

    async fn handle_rpc(&self, method: &str, params: Value) -> Result<Value> {
        match method {
            "get_tools" => Ok(json!([
                {
                    "name": "analyze_python",
                    "description": "Analyze Python code",
                    "input_schema": {
                        "type": "object",
                        "properties": {"code": {"type": "string"}},
                        "required": ["code"]
                    }
                }
            ])),
            "execute_tool" => {
                let name = params["name"].as_str().unwrap_or("");
                let args = params.get("args").cloned().unwrap_or_default();
                match name {
                    "analyze_python" => {
                        let code = args["code"].as_str().unwrap();
                        let errors = analyze(code)?;
                        Ok(json!({"errors": errors}))
                    }
                    _ => Err(Error::method_not_found(name)),
                }
            }
            _ => Err(Error::method_not_found(method)),
        }
    }
}
```

### Example 2: Timeline Widget Extension

A scribe-rpc example that provides timeline events:

```rust
#[async_trait]
impl Extension for TimelineExt {
    async fn handle_rpc(&self, method: &str, _params: Value) -> Result<Value> {
        match method {
            "get_timeline" => Ok(json!({
                "events": [
                    {
                        "title": "Session Started",
                        "timestamp": "2024-03-31T14:00:00Z",
                        "description": "User opened Omegon"
                    }
                ]
            })),
            _ => Err(Error::method_not_found(method)),
        }
    }
}
```

See [scribe-rpc](https://github.com/styrene-lab/scribe-rpc) for the full implementation.

## Published Extensions

First-party extensions available for immediate install:

### Vox — Communication Connector

Unified interface for email, Signal, Slack, Discord, and more. Inbound messages arrive as agent prompts; replies route back to the originating channel.

```sh
omegon extension install https://github.com/styrene-lab/vox.git
```

Tools: `vox_reply`, `vox_send`, `vox_channels` — [GitHub](https://github.com/styrene-lab/vox)

### Scry — Local Image Generation

Text-to-image, image-to-image, and upscaling using local diffusion models (FLUX, SDXL, SD1.5). All inference runs on-device via ComfyUI.

```sh
omegon extension install https://github.com/styrene-lab/scry.git
```

Tools: `generate`, `refine`, `upscale`, `list_models`, `search_models`, `download_model` — [GitHub](https://github.com/styrene-lab/scry)

### Omegon Browser — Browser Automation

Native extension wrapper around Vercel `agent-browser` for controlled browser automation, snapshots, clicks, form fills, waits, screenshots, and command batches.

```sh
omegon extension install omegon-browser
```

Tools: `browser_status`, `browser_open`, `browser_snapshot`, `browser_click`, `browser_fill`, `browser_wait`, `browser_get`, `browser_screenshot`, `browser_batch`

## Manifest Reference

Required sections in `manifest.toml`:

```toml
[extension]
name = "my-extension"       # Lowercase, alphanumeric + hyphens
version = "0.1.0"           # Semantic version
description = "..."         # Optional

[runtime]
type = "native"             # "native" or "oci"
binary = "target/release/my-extension"  # For native
# OR
# image = "registry/image:tag"  # For OCI

[startup]
ping_method = "get_tools"   # Method to call for health check
timeout_ms = 5000           # Health check timeout

[widgets.my_widget]         # Optional, one per widget
label = "My Widget"         # Tab label
kind = "stateful"           # "stateful" or "ephemeral"
renderer = "timeline"       # Widget type
```

## Troubleshooting

### Extension not discovered

- Check path: `~/.omegon/extensions/{name}/manifest.toml`
- Check manifest is valid TOML: `toml-cli ~/.omegon/extensions/{name}/manifest.toml`
- Check extension name matches directory name (or close enough)

### Extension fails health check

- Ensure binary is built and executable
- Run binary manually, send `{"jsonrpc":"2.0","id":"1","method":"get_tools","params":{}}`
- Check that `startup.ping_method` exists and returns success
- Increase `startup.timeout_ms` if extension needs more time

### RPC calls hang

- Ensure `handle_rpc` doesn't block forever
- Use `tokio::time::timeout()` for long operations
- Check extension logs: `RUST_LOG=debug,my_extension=trace`

### Type validation fails

- Validate incoming params: `.as_str()`, `.as_object()`, etc.
- Use `serde_json::json!()` for responses
- Check error messages — serde reports type mismatches clearly

## Version Compatibility

Each Omegon release ships with a matching `omegon-extension` SDK crate. Rust extensions should depend on the SDK version they target, or on a source checkout during local development.

The current manifest schema does not require an `sdk_version` field. Compatibility is enforced by the extension protocol and by build-time dependency selection rather than a manifest-level version gate.

**Breaking changes** (next major version):
- New required RPC methods
- Changed error codes
- Removed features

**Non-breaking changes** (minor version):
- New optional RPC methods
- New optional manifest fields
- New error codes (old code still works)

## Contributing to Omegon Extension SDKs

The Rust extension SDK is now owned outside the host repository in
[`styrene-lab/omegon-extension-rs`](https://github.com/styrene-lab/omegon-extension-rs)
and published as `omegon-extension` on crates.io. To contribute SDK changes:

1. Fork `styrene-lab/omegon-extension-rs`.
2. Create a feature branch: `git checkout -b feature/extension-xyz`.
3. Make SDK changes in the standalone SDK repository.
4. Run `cargo test --features test-extension-bin` and `cargo publish --dry-run`.
5. If the wire contract changes, update `schema/sdk-contract.json` and port the
   artifact to the Python and TypeScript SDKs.
6. Open a PR with the SDK contract impact called out explicitly.

Host runtime changes still belong in `styrene-lab/omegon`. The host consumes the
published SDK crate and should validate extension compatibility using
`SDK_CONTRACT_VERSION`, not the Rust crate version.

## Resources

- **SDK Quick Start**: [EXTENSION_SDK.md](./EXTENSION_SDK.md)
- **Advanced Patterns**: [EXTENSION_INTEGRATION.md](./EXTENSION_INTEGRATION.md)
- **Example Implementation**: [scribe-rpc](https://github.com/styrene-lab/scribe-rpc)
- **Vox Extension**: [styrene-lab/vox](https://github.com/styrene-lab/vox) — communication connector
- **Scry Extension**: [styrene-lab/scry](https://github.com/styrene-lab/scry) — local image generation
- **API Documentation**: `cargo doc -p omegon-extension --open`
- **Issues & Discussions**: [GitHub Issues](https://github.com/styrene-lab/omegon/issues)

## Roadmap

Future enhancements:

- [x] Extension armory/registry commands (`omegon extension list --available`, `omegon extension search`)
- [ ] Manifest-level minimum Omegon version constraints
- [ ] Hot-reload without TUI restart
- [ ] Shared extension dependencies (monorepo support)
- [ ] gRPC transport option (lower latency than JSON-RPC)
- [ ] Binary SDK for Go, Python, TypeScript

---

Extension failures should not crash Omegon. If an extension stops responding, Omegon disables that extension path and keeps the host process running.
