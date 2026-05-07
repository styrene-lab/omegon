+++
id = "2e89146a-ccbb-48b4-93d9-e8a038d7239a"
kind = "design_node"
title = "Rust-Native Extension Boundary — Sidecar Protocol and Migration Path"
status = "decided"
tags = ["rust", "architecture", "extensions", "ipc", "protocol"]
aliases = ["rust-native-extension-boundary"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
dependencies = []
open_questions = []
+++

# Rust-Native Extension Boundary — Sidecar Protocol and Migration Path

## Overview

All omegon-native extensions are written in Rust. The omegon extension host is TypeScript/Node.js. This document defines the interface boundary between them: what the TypeScript layer is responsible for, what the Rust binary owns, how they communicate, and how that boundary retreats toward a fully Rust-native host over time.

This pattern is established with Scribe (engagement management) and becomes the canonical template for all future omegon-native extensions.

## The Constraint

The pi extension TUI API (`@styrene-lab/pi-tui`) renders via TypeScript objects inside the omegon Node.js process. `ctx.ui.setFooter()`, `ctx.ui.custom()`, `ctx.ui.setWidget()` — all take TypeScript `Component` instances. Rust cannot call these directly. The TypeScript layer is therefore unavoidable at this stage of omegon's architecture.

This is not a problem — it forces a clean separation:

| Layer | Owner | Responsibility |
|-------|-------|----------------|
| `.scribe/` reads/writes | Rust | File I/O, business logic, data types |
| Git operations | Rust | Shell-out + `gix`, uses native auth |
| Context resolution | Rust | "Which engagement am I in?" — pure logic |
| Filesystem watching | Rust | `notify` crate, push notifications to TS |
| Background sync | Rust | Tokio tasks, no TS involvement |
| All data types | Rust | Serialized via `serde_json` across the boundary |
| JSON-RPC transport | TypeScript | Spawn, buffer, parse, dispatch |
| pi-tui rendering | TypeScript | `ctx.ui.*` APIs are TypeScript-only |
| Command/tool registration | TypeScript | `pi.registerCommand()` / `pi.registerTool()` |

**The TypeScript layer is render-only: IPC transport + pi-tui components, zero business logic.** If you stripped the TypeScript out, the Rust binary would still be a fully functional CLI tool. The TypeScript is the omegon-specific skin on top of a standalone Rust program.

## IPC: Long-Running Sidecar, JSON-RPC 2.0 over stdio

TypeScript spawns the Rust binary once on `session_start`, kills it on `session_shutdown`. Communication is newline-delimited JSON (ndjson) over stdin/stdout.

**Why this transport:**
- Persistent state across commands (active context, file watchers, background sync)
- Bidirectional: Rust pushes unsolicited notifications to TypeScript (context changed, sync complete)
- Single startup cost — not per-command invocation
- Proven at scale: LSP uses this exact model for every major editor integration

**Why ndjson over LSP-style `Content-Length` framing:** simpler to implement, simpler to debug, sufficient for this payload size.

**Alternatives ruled out:**
- *Per-invocation CLI* — no persistent state, no reactive updates, ~50ms cold start per command
- *Unix socket* — no meaningful benefit for a single-client, single-server tool
- *WASM in Node.js* — no filesystem access, no shell-out, `notify`/`tokio`/`std::process` all fail in WASM

### Wire Format

```
// TypeScript → Rust (request)
{"jsonrpc":"2.0","id":1,"method":"get_context","params":{"cwd":"/path/to/repo"}}

// Rust → TypeScript (response)
{"jsonrpc":"2.0","id":1,"result":{"partnership":"qrypt","engagement":"QRYPT-PLATFORM-001"}}

// Rust → TypeScript (notification — no id)
{"jsonrpc":"2.0","method":"context_changed","params":{"cwd":"/path","partnership":"qrypt"}}

// Error
{"jsonrpc":"2.0","id":2,"error":{"code":-32602,"message":"engagement not found"}}
```

### TypeScript Sidecar Transport (~100 lines)

```typescript
import { spawn, ChildProcess } from "child_process";
import { EventEmitter } from "events";

export class RpcSidecar extends EventEmitter {
  private proc: ChildProcess | null = null;
  private pending = new Map<number, { resolve: Function; reject: Function }>();
  private nextId = 1;
  private buffer = "";

  start(binaryPath: string, args: string[] = ["--rpc"]) {
    this.proc = spawn(binaryPath, args, { stdio: ["pipe", "pipe", "pipe"] });
    this.proc.stdout!.on("data", (chunk: Buffer) => this.handleData(chunk));
    this.proc.stderr!.on("data", (chunk: Buffer) => {
      // stderr is tracing/logging from Rust — write to file, never to terminal
    });
    this.proc.on("exit", (code) => this.emit("exit", code));
  }

  async request<T>(method: string, params: unknown): Promise<T> {
    const id = this.nextId++;
    this.proc!.stdin!.write(
      JSON.stringify({ jsonrpc: "2.0", id, method, params }) + "\n"
    );
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
    });
  }

  shutdown() {
    this.request("shutdown", {}).finally(() => this.proc?.kill("SIGTERM"));
  }

  private handleData(chunk: Buffer) {
    this.buffer += chunk.toString();
    const lines = this.buffer.split("\n");
    this.buffer = lines.pop()!;
    for (const line of lines) {
      if (line.trim()) this.dispatch(JSON.parse(line));
    }
  }

  private dispatch(msg: any) {
    if (msg.id != null) {
      const p = this.pending.get(msg.id);
      if (p) {
        this.pending.delete(msg.id);
        msg.error ? p.reject(msg.error) : p.resolve(msg.result);
      }
    } else {
      this.emit(msg.method, msg.params); // notification
    }
  }
}
```

This class lives in `extensions/lib/rpc-sidecar.ts` and is shared by all Rust-native extensions.

### Rust RPC Loop

```rust
use tokio::io::{AsyncBufReadExt, BufReader};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct Request {
    id: Option<u64>,
    method: String,
    params: serde_json::Value,
}

#[derive(Serialize)]
struct Response<T: Serialize> {
    jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

pub async fn run_rpc_loop(state: Arc<AppState>) -> anyhow::Result<()> {
    let reader = BufReader::new(tokio::io::stdin());
    let mut lines = reader.lines();

    while let Some(line) = lines.next_line().await? {
        let req: Request = serde_json::from_str(&line)?;
        let response = dispatch(&state, req).await;
        println!("{}", serde_json::to_string(&response)?);
    }
    Ok(())
}
```

## Dual-Mode Binary

The Rust binary is a standard CLI without `--rpc`, and enters the JSON-RPC loop with it:

```
scribe-rpc log                          # CLI: write a log entry interactively
scribe-rpc sync                         # CLI: pull latest from all hub repos
scribe-rpc --rpc                        # Sidecar: JSON-RPC over stdio for omegon
```

This means the binary has three independent consumers from the same logic:
1. **omegon extension** — via `--rpc` sidecar
2. **Terminal user** — via bare CLI (no omegon required)
3. **Dashboard tier** (future) — via library crate, same business logic, different interface

One Rust artifact. The `src/` layout reflects this:

```
{extension}-rpc/src/
├── main.rs          — entry point: --rpc → rpc::run(), else → cli::run()
├── rpc/
│   └── dispatch.rs  — JSON-RPC handler dispatch
├── cli/
│   └── commands.rs  — clap subcommands
└── {domain}/        — all business logic (no awareness of transport)
    ├── mod.rs
    └── ...
```

## Extension File Structure

```
extensions/
├── lib/
│   └── rpc-sidecar.ts          ← shared transport (written once, used by all)
│
├── scribe/                     ← first omegon-native extension
│   ├── index.ts                ← command/tool/hook registration (~100 lines)
│   ├── sidecar.ts              ← typed wrapper around RpcSidecar
│   ├── components/
│   │   ├── footer.ts           ← partnership/engagement context in footer
│   │   ├── engagement-picker.ts
│   │   └── log-composer.ts
│   └── scribe-rpc/             ← Rust workspace
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs
│           ├── rpc/
│           ├── cli/
│           ├── scribe/         ← .scribe/ format, log entry, session log
│           ├── git/            ← gix reads + shell-out writes
│           └── sync/           ← notify watcher + background pull
│
└── {future-extension}/
    ├── index.ts
    └── {future-extension}-rpc/
```

Each future extension is: `extensions/lib/rpc-sidecar.ts` (already written) + thin TypeScript adapter + Rust binary. The pattern is mechanical to replicate.

## The Boundary Retreats — Migration Path

The sidecar is Phase 1 of a coherent migration toward a fully Rust-native host. Each phase requires **zero changes to the Rust business logic** — only the calling convention changes.

### Phase 1 (now): Subprocess + JSON-RPC over stdio

```
omegon (Node.js)
  └── extensions/scribe/index.ts      TypeScript adapter (~200 lines)
        ↕  stdin/stdout ndjson
  scribe-rpc (subprocess)             Rust binary, --rpc mode
```

The TypeScript adapter is plumbing. All logic is in Rust.

---

### Phase 2: napi-rs native addon (.node)

[**Trigger:** omegon adds napi-rs to its build pipeline, or a specific extension has performance requirements that make subprocess overhead unacceptable.]

The Rust logic compiles to a `.node` native addon loaded directly into the omegon Node.js process. No subprocess, no JSON serialization, no buffer management. Direct FFI calls.

```
omegon (Node.js)
  └── extensions/scribe/index.ts      TypeScript adapter shrinks to ~50 lines of type bindings
        ↕  direct FFI calls
  scribe_rpc.node (in-process)        same Rust logic, different entry point
```

**What changes in the Rust codebase:**
```toml
# Cargo.toml — additive, binary target still exists
[lib]
crate-type = ["cdylib"]

[dependencies]
napi = { version = "2", features = ["napi6"] }
napi-derive = "2"
```

```rust
// src/lib.rs — thin napi wrapper around existing functions
use napi_derive::napi;

#[napi]
pub fn get_context(cwd: String) -> napi::Result<ContextResult> {
    scribe::resolve_context(&cwd)
        .map_err(|e| napi::Error::from_reason(e.to_string()))
}
```

The napi surface is a thin wrapper around the same functions the JSON-RPC dispatcher calls. The business logic in `src/scribe/`, `src/git/`, `src/sync/` is untouched.

**What changes in TypeScript:**
```typescript
// Before (Phase 1): IPC request
const ctx = await sidecar.request<Context>("get_context", { cwd });

// After (Phase 2): direct FFI call
import { getContext } from "./scribe_rpc.node";
const ctx = getContext(cwd);
```

TypeScript adapter: 200 lines → ~50 lines.

---

### Phase 3: Omegon runtime migrates to Rust

[**Trigger:** the omegon runtime itself moves toward a Rust-first architecture, pi-tui rendering becomes native.]

The TypeScript adapter disappears entirely. Extensions implement a Rust trait. The host API is a set of Rust function calls.

```
omegon (Rust runtime)
  └── extensions/scribe/              pure Rust, no TypeScript
        implements ExtensionTrait
```

```rust
// Hypothetical future API
impl Extension for ScribeExtension {
    fn register(&self, host: &mut dyn ExtensionHost) {
        host.register_command("scribe:log", |ctx| self.cmd_log(ctx));
        host.set_footer(Box::new(ScribeFooter::new(self.state.clone())));
    }
}
```

**What changes:** the extension's entry point and rendering calls. **What doesn't change:** every line of business logic in `src/scribe/`, `src/git/`, `src/sync/`. Zero migration cost for the actual logic.

---

### Why the current design enables this

The architectural property that makes each phase a mechanical swap rather than a structural refactor:

**Zero business logic in TypeScript.** The adapter transports bytes and renders data it receives. When the transport mechanism changes (subprocess → FFI → native), the business logic doesn't move because it was never in the adapter. The nouns and verbs of the protocol (`get_context`, `write_log_entry`, `list_partnerships`) remain constant — only the calling convention changes.

The dual-mode binary (`--rpc` + bare CLI) also means the `cdylib` output in Phase 2 is purely additive — `crate-type = ["cdylib", "bin"]` exposes both interfaces from the same logic without restructuring.

## Decisions

### IPC: long-running Rust sidecar, JSON-RPC 2.0 over stdin/stdout (ndjson)

**Status:** decided

TypeScript cannot be eliminated (pi-tui rendering is TypeScript-native), so the boundary is unavoidable. Sidecar over stdio is proven (LSP is the same model), supports persistent state and bidirectional notifications, and has near-zero overhead after startup. WASM is ruled out (no filesystem access, no shell-out, no async I/O). Per-invocation CLI calls are ruled out (no persistent state, no reactive updates). ndjson (no `Content-Length` framing) is simpler than LSP wire format for this use case.

### TypeScript layer is render-only: IPC transport + pi-tui components, zero business logic

**Status:** decided

All data reading, writing, git operations, context resolution, and sync logic live in Rust. TypeScript only: spawns/kills the process, serializes/deserializes JSON-RPC messages, and maps response data into pi-tui component trees. This makes the Rust binary independently useful as a CLI and ensures the TypeScript adapter is small enough (~200 lines) that it becomes a template pattern for future extensions.

### Binary is dual-mode: `--rpc` flag for omegon sidecar, bare CLI for standalone use

**Status:** decided

The Rust binary without `--rpc` is a standard CLI tool. With `--rpc` it enters the JSON-RPC loop. The omegon extension is one consumer, terminal use without omegon is another, and the dashboard tier (if built) is a third consumer via library crate. One Rust artifact, multiple interfaces.

### This sidecar pattern is the canonical template for all future omegon-native extensions

**Status:** decided

Every future omegon-native Rust extension follows: `extensions/{name}/index.ts` (adapter) + `extensions/{name}/{name}-rpc/` (Rust binary). The shared `extensions/lib/rpc-sidecar.ts` is written once with Scribe and reused. Future extensions only need the Rust logic and a thin TypeScript render layer.

### The boundary is explicitly designed to retreat

**Status:** decided

Phase 1 (subprocess) → Phase 2 (napi-rs FFI) → Phase 3 (native Rust host). Each phase requires zero changes to Rust business logic. The migration is mechanical because the TypeScript adapter contains no logic to preserve. This is not an accident — it is the reason for the "zero business logic in TypeScript" rule.
