# Omegon Extension Integration Guide

This guide covers advanced patterns for extensions built with the Omegon Extension SDK.

## Table of Contents

1. [Providing Tools](#providing-tools)
2. [Widget Patterns](#widget-patterns)
3. [State Management](#state-management)
4. [Performance Optimization](#performance-optimization)
5. [Testing Extensions](#testing-extensions)
6. [Publishing](#publishing)

---

## Providing Tools

Tools are RPC methods that extend Omegon's capabilities. Each tool declares its input schema (JSON Schema).

### Declaring Tools

Implement `get_tools` to return tool definitions:

```rust
async fn handle_rpc(&self, method: &str, params: Value) -> Result<Value> {
    match method {
        "get_tools" => {
            Ok(json!([
                {
                    "name": "analyze_python",
                    "description": "Analyze Python code for type errors, style issues, and security problems",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "code": {"type": "string"},
                            "check_types": {"type": "boolean", "default": true},
                            "check_security": {"type": "boolean", "default": true}
                        },
                        "required": ["code"]
                    }
                }
            ]))
        }
        _ => Err(Error::method_not_found(method)),
    }
}
```

### Implementing Tools

When a user invokes a tool, Omegon calls `execute_{tool_name}`:

```rust
async fn handle_rpc(&self, method: &str, params: Value) -> Result<Value> {
    match method {
        "execute_analyze_python" => {
            let code = params.get("code")
                .and_then(|v| v.as_str())
                .ok_or_else(|| Error::invalid_params("expected 'code' string"))?;
            
            let check_types = params.get("check_types")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            
            // Do the actual work
            let errors = analyze_code(code, check_types).await?;
            
            Ok(json!({
                "status": "success",
                "error_count": errors.len(),
                "errors": errors
            }))
        }
        _ => Err(Error::method_not_found(method)),
    }
}

async fn analyze_code(code: &str, check_types: bool) -> Result<Vec<Error>> {
    // Your implementation here
    Ok(vec![])
}
```

### Tool Response Format

While the input schema is validated by Omegon, the response format is flexible. Return whatever data makes sense for your tool:

```json
{
  "status": "success",
  "result": { "data": "..." },
  "timing": { "analysis_ms": 150, "inference_ms": 200 }
}
```

Or for streaming-like behavior, include multiple results:

```json
{
  "status": "success",
  "results": [
    { "file": "main.py", "issues": 3 },
    { "file": "util.py", "issues": 1 }
  ]
}
```

---

## Widget Patterns

Widgets are tabs/panels in the Omegon UI. They request data via `get_{widget_id}` and display it.

### Timeline Widget

The timeline widget displays a chronological list of events.

**Manifest:**
```toml
[widgets.timeline]
label = "Timeline"
kind = "stateful"
renderer = "timeline"
```

**Response from `get_timeline`:**
```json
{
  "events": [
    {
      "title": "User Onboarding Started",
      "timestamp": "2024-03-31T14:30:00Z",
      "description": "User activated their account and began setup"
    },
    {
      "title": "API Key Generated",
      "timestamp": "2024-03-31T14:35:00Z",
      "description": "Created API key for integration testing"
    }
  ]
}
```

Events are rendered in reverse chronological order. Include:
- `title` (required): short summary
- `timestamp` (required): ISO 8601 format
- `description` (optional): detailed explanation
- `metadata` (optional): custom fields for filtering/grouping

### Memory Widget

Displays structured memory/knowledge from the extension.

**Manifest:**
```toml
[widgets.memory]
label = "Knowledge"
kind = "stateful"
renderer = "tree"
```

**Response from `get_memory`:**
```json
{
  "facts": [
    {
      "id": "fact-001",
      "section": "Architecture",
      "content": "Extensions run as isolated processes via RPC",
      "tags": ["extension-sdk", "safety"],
      "confidence": 0.95
    },
    {
      "id": "fact-002",
      "section": "Patterns",
      "content": "Use prefix matching for SDK version compatibility",
      "related": ["fact-001"],
      "timestamp": "2024-03-31T14:00:00Z"
    }
  ]
}
```

### Custom Widgets

You can define custom widget types. Just declare them in the manifest and return appropriate data.

**Manifest:**
```toml
[widgets.analysis]
label = "Code Analysis"
kind = "stateful"
renderer = "custom:analysis"
```

The renderer name is arbitrary — you can define custom rendering logic in frontend extensions or use generic table/tree layouts.

---

## State Management

Extensions are stateless from Omegon's perspective (no persistence). However, you can manage internal state:

### In-Memory State

Use a shared `Arc<Mutex<State>>`:

```rust
use std::sync::Arc;
use tokio::sync::Mutex;

struct MyExtension {
    cache: Arc<Mutex<HashMap<String, Value>>>,
}

#[async_trait]
impl Extension for MyExtension {
    async fn handle_rpc(&self, method: &str, params: Value) -> Result<Value> {
        match method {
            "get_data" => {
                let cache = self.cache.lock().await;
                if let Some(cached) = cache.get("key") {
                    return Ok(cached.clone());
                }
                // Compute and cache
                drop(cache);
                let result = compute_expensive_work(&params).await?;
                self.cache.lock().await.insert("key".to_string(), result.clone());
                Ok(result)
            }
            _ => Err(Error::method_not_found(method)),
        }
    }
}
```

### Persistent State

For extensions that need to persist data across restarts:

```rust
use std::fs;
use std::path::PathBuf;

struct MyExtension {
    data_dir: PathBuf,
}

impl MyExtension {
    fn state_file(&self) -> PathBuf {
        self.data_dir.join("state.json")
    }

    async fn load_state(&self) -> Result<Value> {
        let content = fs::read_to_string(self.state_file())?;
        Ok(serde_json::from_str(&content)?)
    }

    async fn save_state(&self, state: &Value) -> Result<()> {
        let content = serde_json::to_string_pretty(&state)?;
        fs::write(self.state_file(), content)?;
        Ok(())
    }
}
```

**Note:** The extension process lifetime is per Omegon session. If Omegon exits, the extension shuts down. If you need persistent data, use the filesystem.

---

## Performance Optimization

### Timeout Tuning

Set `startup.timeout_ms` in manifest based on your extension's startup time:

```toml
[startup]
ping_method = "get_tools"
timeout_ms = 10000  # 10 seconds for heavy startup (e.g., model loading)
```

Omegon allows up to 60 seconds; higher values slow TUI startup.

### Lazy Loading

Defer expensive initialization to the first actual use:

```rust
struct MyExtension {
    model: Arc<Mutex<Option<HeavyModel>>>,
}

async fn load_model(&self) -> Result<&HeavyModel> {
    let mut model = self.model.lock().await;
    if model.is_none() {
        *model = Some(HeavyModel::load().await?);
    }
    Ok(model.as_ref().unwrap())
}

async fn handle_rpc(&self, method: &str, params: Value) -> Result<Value> {
    match method {
        "get_tools" => {
            // Light operation — return immediately
            Ok(json!([...]))
        }
        "analyze" => {
            // Heavy operation — lazy load the model
            let model = self.load_model().await?;
            let result = model.analyze(&params)?;
            Ok(result)
        }
        _ => Err(Error::method_not_found(method)),
    }
}
```

### Streaming-Like Responses

For large datasets, return paginated results:

```rust
async fn handle_rpc(&self, method: &str, params: Value) -> Result<Value> {
    match method {
        "get_timeline" => {
            let page = params.get("page").and_then(|v| v.as_u64()).unwrap_or(0);
            let page_size = 50;
            
            let events = fetch_events(page * page_size, page_size).await?;
            let total = total_event_count().await?;
            
            Ok(json!({
                "events": events,
                "page": page,
                "page_size": page_size,
                "total": total,
                "has_more": (page + 1) * page_size < total
            }))
        }
        _ => Err(Error::method_not_found(method)),
    }
}
```

---

## Testing Extensions

### Unit Tests

Test each RPC method individually:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_tools() {
        let ext = MyExtension::default();
        let result = ext.handle_rpc("get_tools", json!({})).await;
        
        let tools = result.unwrap();
        assert!(tools.is_array());
        assert!(tools.as_array().unwrap().len() > 0);
    }

    #[tokio::test]
    async fn test_invalid_params() {
        let ext = MyExtension::default();
        let result = ext.handle_rpc("execute_analyze", json!({})).await;
        
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), ErrorCode::InvalidParams);
    }

    #[tokio::test]
    async fn test_unknown_method() {
        let ext = MyExtension::default();
        let result = ext.handle_rpc("unknown_method", json!({})).await;
        
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), ErrorCode::MethodNotFound);
    }
}
```

### Integration Tests

Test the full RPC loop by spawning the extension:

```bash
# Build extension
cargo build --release

# Test with omegon
echo '{"jsonrpc": "2.0", "id": "1", "method": "get_tools", "params": {}}' | \
  ./target/release/my-extension
```

### Testing with Omegon

1. Install extension to `~/.omegon/extensions/{name}/`
2. Run Omegon with `RUST_LOG=debug` to see extension logs
3. Manually invoke tools and check widget data

```bash
RUST_LOG=debug,omegon_extension=trace cargo run --release
```

---

## Publishing

### Prepare Your Extension

1. **Version Cargo.toml** with semantic versioning:
   ```toml
   [package]
   name = "my-omegon-extension"
   version = "0.1.0"
   ```

2. **SDK version in Cargo.toml** (when released):
   ```toml
   [dependencies]
   omegon-extension = "0.15.6"
   ```

3. **Create manifest.toml**:
   ```toml
   [extension]
   name = "my-extension"
   version = "0.1.0"
   description = "My custom extension"
   sdk_version = "0.15"
   
   [runtime]
   type = "native"
   binary = "target/release/my-extension"
   
   [startup]
   ping_method = "get_tools"
   timeout_ms = 5000
   
   [widgets.custom]
   label = "Custom"
   kind = "stateful"
   renderer = "custom:mywidget"
   ```

4. **Add README.md** with:
   - Description
   - Installation instructions
   - Example usage
   - Tool reference

5. **Test the manifest** locally:
   ```bash
   mkdir -p ~/.omegon/extensions/my-extension
   cp target/release/my-extension manifest.toml ~/.omegon/extensions/my-extension/
   cargo run --release  # from omegon directory
   ```

### GitHub Release

1. Tag your release:
   ```bash
   git tag -a v0.1.0 -m "Initial release"
   git push origin v0.1.0
   ```

2. Build and upload binary:
   ```bash
   cargo build --release
   gh release create v0.1.0 ./target/release/my-extension
   ```

3. Users install from your GitHub release:
   ```bash
   mkdir -p ~/.omegon/extensions/my-extension
   curl -L https://github.com/your/repo/releases/download/v0.1.0/my-extension \
     -o ~/.omegon/extensions/my-extension/my-extension
   chmod +x ~/.omegon/extensions/my-extension/my-extension
   cp manifest.toml ~/.omegon/extensions/my-extension/
   ```

### OCI Container Release

For OCI container extensions:

1. Build and push to registry:
   ```bash
   docker build -t your-registry/my-extension:0.1.0 .
   docker push your-registry/my-extension:0.1.0
   ```

2. Update manifest.toml:
   ```toml
   [runtime]
   type = "oci"
   image = "your-registry/my-extension:0.1.0"
   ```

3. Users install with manifest only:
   ```bash
   mkdir -p ~/.omegon/extensions/my-extension
   cp manifest.toml ~/.omegon/extensions/my-extension/
   # Omegon will podman pull the image on first startup
   ```

---

## Next Steps

- Read [EXTENSION_SDK.md](./EXTENSION_SDK.md) for API reference
- Clone the [scribe-rpc example extension](https://github.com/styrene-lab/scribe-rpc)
- Join the Omegon community Discord for questions

Happy building! 🚀
