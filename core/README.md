# omegon-core

Rust-native agent loop and lifecycle engine for [Omegon](https://github.com/styrene-lab/omegon).

## Architecture

A single Rust binary (`omegon-agent`) that owns the agent loop, lifecycle engine, and core tools. LLM provider access is through a ~100-line Node.js subprocess bridge that imports `@styrene-lab/pi-ai`.

```
omegon-agent (Rust)
  ├── Agent Loop — state machine, steering, follow-up
  ├── Lifecycle Engine — explore → specify → decompose → implement → verify
  │     ├── Ambient capture (omg: XML tags from agent reasoning)
  │     ├── Autonomous decomposition (above complexity threshold)
  │     └── sqlite lifecycle store (.pi/lifecycle.db)
  ├── ContextManager — dynamic per-turn system prompt injection
  ├── ConversationState — context decay, IntentDocument
  ├── Core Tools — understand, change, execute, remember, speculate
  └── Feature Crates — memory, render, view, search, ollama, mcp
        ↕ ToolProvider / ContextProvider / EventSubscriber / SessionHook
```

## Phases

- **Phase 0**: Headless agent loop for cleave children. First production consumer.
- **Phase 1**: Process owner. Node.js subprocesses for LLM bridge + TUI bridge.
- **Phase 2**: Native TUI (ratatui). Node.js only for LLM bridge.
- **Phase 3**: Native LLM clients (reqwest). Node.js for long-tail providers only.

## Development

```bash
cargo build
cargo test
cargo run -- --prompt "Fix the typo in main.rs" --cwd /path/to/repo
```

## License

MIT
