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
omegon-extension = "0.16"
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
            "get_tools" => Ok(json!([])),
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
