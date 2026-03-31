# Scribe RPC — First Rust-Native Omegon Extension

This is the canonical implementation of the **Rust-native extension** pattern for omegon. Scribe is fully integrated into the Rust runtime via the `Feature` trait.

## Architecture

```
omegon (Rust binary)
  │
  ├─ setup.rs: Create ScribeFeature, spawn scribe-rpc sidecar
  │
  ├─ features/scribe.rs (ScribeFeature)
  │   └─ Implements Feature trait
  │   └─ Registers 3 tools: scribe_context, scribe_log, scribe_list
  │   └─ Manages RPC communication
  │
  ↕ ndjson (JSON-RPC 2.0) over stdin/stdout
  │
  └─ core/crates/scribe-rpc/ (Rust binary, spawned as sidecar)
     ├─ main.rs            entry point: --rpc flag for sidecar mode
     ├─ rpc/
     │  ├─ mod.rs          JSON-RPC loop, message parsing
     │  └─ dispatch.rs     method handlers → business logic
     ├─ cli/
     │  └─ mod.rs          standalone CLI mode (scribe log, status, sync)
     └─ scribe/
        └─ mod.rs          business logic, transport-agnostic
```

## Running

### Build

```bash
cd core
cargo build -p scribe-rpc --release
```

Binary: `core/target/release/scribe-rpc`

### Standalone CLI

```bash
scribe-rpc log "Fixed timeout bug" --category development
scribe-rpc status
scribe-rpc sync
```

### Sidecar Mode (omegon)

```bash
scribe-rpc --rpc
```

Listens on stdin for ndjson RPC requests, writes responses to stdout.

## RPC Methods

### get_context

**Request:**
```json
{"jsonrpc":"2.0","id":1,"method":"get_context","params":{"cwd":"/path/to/repo"}}
```

**Response:**
```json
{
  "jsonrpc":"2.0",
  "id":1,
  "result":{
    "partnership":"qrypt",
    "engagement_id":"QRYPT-001",
    "team_members":["alice","bob"],
    "recent_activity":["PR merged","deployment"]
  }
}
```

### get_status

Returns engagement status, progress, and last update time.

```json
{"jsonrpc":"2.0","id":2,"method":"get_status","params":{"cwd":"."}}
```

### write_log

Add a work log entry to the engagement.

```json
{
  "jsonrpc":"2.0",
  "id":3,
  "method":"write_log",
  "params":{
    "content":"Completed integration tests",
    "category":"development"
  }
}
```

### get_timeline

Fetch engagement timeline (commits, PRs, manual logs).

```json
{"jsonrpc":"2.0","id":4,"method":"get_timeline","params":{"cwd":".","page":1,"per_page":20}}
```

### shutdown

Graceful shutdown signal (sent by omegon on session end).

```json
{"jsonrpc":"2.0","id":5,"method":"shutdown","params":{}}
```

## Notifications

Rust can push unsolicited notifications to omegon (e.g., when engagement context changes).

```json
{"jsonrpc":"2.0","method":"context_changed","params":{"partnership":"new-partner"}}
```

## Implementation Status

### Complete (Phase 3: Pure Rust)

- ✅ Rust binary: main.rs, RPC loop, method dispatch, CLI mode
- ✅ ScribeFeature: Feature trait implementation, tool registration, RPC communication
- ✅ omegon integration: Spawned in setup.rs, registered with EventBus
- ✅ Tools: scribe_context, scribe_log, scribe_list (callable by agent)
- ✅ Build: Integrated into workspace, binary included in release

### TODO: Backend Implementation

- [ ] .scribe file format (TOML) — read engagement metadata
- [ ] HTTP client — call SCRIBE_URL endpoints (reqwest)
- [ ] Token caching — refresh engagement context every 30 turns
- [ ] Filesystem watcher (notify crate) — push notifications on changes
- [ ] Git integration — read recent commits, associate with logs
- [ ] Session lifecycle hooks — sync on session start/end

## Design Patterns

### Transport-Agnostic Business Logic

All functions in `scribe-rpc/src/scribe/` are pure async functions with no transport awareness:
- Same code runs in CLI mode, RPC mode, and future FFI bindings
- No coupling to JSON serialization
- Tests call the business logic directly

### Dual-Mode Binary

```rust
// main.rs
if args.rpc {
    rpc::run_rpc_loop().await?;    // Spawned by omegon, reads JSON-RPC on stdin
} else if let Some(cmd) = args.command {
    cli::execute(cmd).await?;      // Standalone CLI: scribe log, scribe status
}
```

One binary, two consumers: omegon (sidecar) and terminal users.

### Feature-Based Integration

```rust
// features/scribe.rs
#[async_trait]
impl Feature for ScribeFeature {
    fn tools(&self) -> Vec<ToolDefinition> { /* register 3 tools */ }
    async fn execute(&self, tool_name: &str, args: Value, _cancel: Token) -> Result<ToolResult> {
        /* send JSON-RPC request to sidecar */
    }
}
```

No intermediate adapters. Rust talks directly to Rust.

## Why This Pattern

**Phase 1 (Historical)**: Omegon was TypeScript + pi. Extensions were plugins loaded at runtime. Scribe was a separate TS extension communicating with a Rust RPC binary.

**Phase 2 (Completed)**: Omegon became Rust. The TypeScript extension layer became unnecessary. Omegon now spawns the RPC sidecar directly and consumes it via the Feature trait.

**Phase 3 (Current)**: Scribe is fully integrated. No bridge, no adapter, no intermediate process. Just Feature trait ↔ RPC binary.

Future Rust-native extensions follow the same pattern:
1. Write business logic in Rust (transport-agnostic)
2. Add RPC dispatch layer (JSON-RPC 2.0 over stdin/stdout)
3. Implement Feature trait in omegon to spawn and communicate
4. Done — no TypeScript layer needed

## Testing

```bash
# Unit tests for business logic (Rust)
cargo test -p scribe-rpc

# Integration test: spawn sidecar, send RPC requests
# (TODO: implement via child_process in tests)

# Standalone CLI smoke tests (TODO)
scribe-rpc log "test entry" --category development
scribe-rpc status
```

## Resources

- **Feature integration**: `core/crates/omegon/src/features/scribe.rs`
- **RPC binary**: `core/crates/scribe-rpc/`
- **RPC spec**: JSON-RPC 2.0 over ndjson (stdin/stdout)
- **Protocol reference**: Method signatures in `scribe-rpc/src/rpc/dispatch.rs`

---

**This is the template for all future omegon-native Rust extensions:**

1. Write business logic in Rust (scribe-rpc/src/scribe/mod.rs)
2. Add RPC dispatch layer (scribe-rpc/src/rpc/dispatch.rs)
3. Add CLI for standalone use (scribe-rpc/src/cli/mod.rs)
4. Implement Feature trait in omegon (features/{name}.rs)
5. Spawn sidecar in setup.rs, register with EventBus
6. Done — pure Rust, no bridge layer needed

No TypeScript. No plugin API. Just Rust traits talking to Rust binaries.
