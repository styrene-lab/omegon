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

## HostActions

HostActions are the SDK contract for host-managed side effects. Extensions describe
intent; Omegon validates every action, applies manifest/runtime/operator policy, and
only then renders or executes it. Returning a HostAction does not make the effect run
by itself.

Use declarative actions in ordinary tool results when the host should present or queue
a side effect with the tool response:

```rust
use omegon_extension::{HostAction, ToolResult};
use omegon_extension::actions::terminal::{TERMINAL_CREATE_V1, TerminalCreateParams};

let params = TerminalCreateParams::new("bookokrat")
    .with_args(["/books/example.epub"]);
let action = HostAction::new("open-reader", TERMINAL_CREATE_V1, params)?;
let result = ToolResult::text("Opening reader").with_action(action);
let value = serde_json::to_value(result)?;
```

The serialized tool result contains ordinary content plus an `actions` array:

```json
{
  "content": [{"type": "text", "text": "Opening reader"}],
  "actions": [{
    "id": "open-reader",
    "type": "terminal.create@1",
    "params": {
      "command": "bookokrat",
      "args": ["/books/example.epub"]
    }
  }]
}
```

Extensions that need the advanced imperative path can use `HostProxy` from `serve_v2()`.
`execute_action()` sends JSON-RPC method `actions/execute` with params
`{"action": <HostAction>}` and returns a full `HostActionOutcome`:

```rust
use omegon_extension::actions::terminal::TerminalCreateResult;

let outcome = host.execute_action(action).await?;
if let Some(result) = outcome.result {
    let terminal: TerminalCreateResult = serde_json::from_value(result)?;
    eprintln!("opened {} via {}", terminal.terminal_id, terminal.backend);
}
```

The expected host response shape is:

```json
{
  "action_id": "open-reader",
  "status": "completed",
  "result": {
    "terminal_id": "term_123",
    "backend": "zellij",
                    "actual_placement": "background_session"
  }
}
```

To advertise support during `initialize`, set the capability flags relevant to the
extension:

```json
{
  "capabilities": {
    "tools": true,
    "host_actions": true,
    "host_action_execution": true
  }
}
```

`terminal.create@1` is SDK/protocol foundation only. The host-side executor, policy
engine, and real terminal process creation live in Omegon core, not in this crate.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT License](LICENSE-MIT) at your option.
