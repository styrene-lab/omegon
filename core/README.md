+++
id = "344718e6-33e8-4640-8f64-f5940bd1fd9b"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# omegon

Rust-native agent harness. See the [root README](../README.md) for usage and installation.

## Architecture

A single Rust binary (`omegon`) that owns the agent loop, lifecycle engine, and core tools. Native HTTPS clients for 11 LLM providers — no Node.js runtime dependency.

```
omegon (Rust)
  ├── Agent Loop — state machine, steering, follow-up
  ├── Lifecycle Engine — explore → specify → decompose → implement → verify
  ├── ContextManager — dynamic per-turn system prompt injection
  ├── ConversationState — context decay, IntentDocument
  ├── Core Tools — read, edit, write, bash, change, commit
  └── Feature Crates — memory, lifecycle, cleave, extensions, ollama
        ↕ ToolProvider / ContextProvider / EventSubscriber / SessionHook
```

## Development

```bash
cargo build
cargo test
cargo run -- --prompt "Fix the typo in main.rs" --cwd /path/to/repo
```

## License

MIT
