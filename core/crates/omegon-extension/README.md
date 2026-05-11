+++
id = "9c355a5d-4fdc-40ee-b87a-891077061acd"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# omegon-extension

Safe, versioned SDK for building [Omegon](https://github.com/styrene-lab/omegon) extensions.

Extensions run as isolated processes communicating via JSON-RPC over stdin/stdout. An extension crash never crashes the host agent.

## Usage

```toml
[dependencies]
omegon-extension = "0.19"
```

```rust
use omegon_extension::{Extension, serve};
use serde_json::{json, Value};

#[derive(Default)]
struct MyExtension;

#[async_trait::async_trait]
impl Extension for MyExtension {
    fn name(&self) -> &str { "my-extension" }
    fn version(&self) -> &str { env!("CARGO_PKG_VERSION") }

    async fn handle_rpc(&self, method: &str, params: Value) -> omegon_extension::Result<Value> {
        match method {
            "get_tools" => Ok(json!([
                {
                    "name": "hello",
                    "description": "Return a greeting",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string"}
                        }
                    }
                }
            ])),
            "execute_tool" => {
                let name = params["name"].as_str().unwrap_or("");
                let args = params.get("args").cloned().unwrap_or_default();
                match name {
                    "hello" => Ok(json!({
                        "content": [{
                            "type": "text",
                            "text": format!(
                                "Hello, {}!",
                                args["name"].as_str().unwrap_or("World")
                            )
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
    serve(MyExtension::default()).await.unwrap();
}
```

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT License](LICENSE-MIT) at your option.
