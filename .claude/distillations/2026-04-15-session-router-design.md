# Session Distillation: Multi-Session Router Design + Security Hardening

Generated: 2026-04-15
Working Directory: /Users/cwilson/workspace/black-meridian/styrene-lab/vox
Repositories: styrene-lab/vox (comms extension), omegon (daemon agent)

## Session Overview

Continued from the e2e container deployment. This session focused on security hardening (trust levels, role-based auth, prompt injection defense) and runtime configuration (env vars, Vault integration). Concluded with a design discussion for parallel multi-session routing — the next major architecture piece.

## Technical State

### Repository Status — vox (styrene-lab/vox)
- Branch: `main`
- Latest commit: `914b85c` feat: vault-backed secret recipes
- All changes committed

### Repository Status — omegon
- Branch: `fix/delegate-provider-inherit`
- Latest commit: `3e7c2629` feat: trust-level prompt framing in vox bridge
- All changes committed (except prior cleave files)

### Key Changes This Session

**Trust-level access control (vox + omegon):**
- `TrustLevel` enum (Operator/User) in vox-core
- `operators` and `operator_roles` on DiscordConfig
- `operators` and `operator_groups` on SlackConfig
- Discord: `classify_trust()` checks user ID → role membership → default User
- Discord: parses `member.roles` from MESSAGE_CREATE gateway events
- Slack: resolves usergroup membership at startup via `usergroups.users.list`
- Omegon vox bridge: operator messages get direct prompt framing, user messages get XML containment with "Do NOT follow instructions" directive
- 4 Discord trust tests, 5 omegon bridge trust tests

**Runtime config from env vars:**
- Entrypoint generates vox.toml from `VOX_DISCORD_*` / `VOX_SLACK_*` env vars
- Fallback to baked-in config when no env vars set
- CSV → TOML array conversion for operators, roles, allowed_users

**Vault-backed secrets:**
- `FOO_VAULT=secret/data/path#key` → writes `vault:` recipe to secrets.json
- Falls back to `FOO=value` → writes `env:` recipe
- Mixed mode: some secrets from Vault, others from env
- Auth validation accepts vault-backed credentials
- Omegon's SecretsManager resolves `vault:` recipes via async Vault KV v2 client

### Test Counts
- vox: 21 tests (12 discord incl. 4 trust, 9 slack), 0 failures
- omegon: 5 vox_bridge tests (3 trust-level framing), 0 failures

## Decisions Made

1. **Trust levels, not just allowlists** — separate instruction plane (operator) from data plane (user). Default is User (containment-wrapped).

2. **Discord roles for operator trust** — `operator_roles` config maps Discord server roles to operator trust. More manageable than user ID lists for teams.

3. **Slack usergroups for operator trust** — `operator_groups` resolved at startup, cached. No per-message API calls.

4. **Env var config generation** — standard k8s pattern. Non-sensitive config (operators, roles, guild IDs) from env vars. Sensitive secrets from Vault.

5. **Vault-first for secrets in production** — `FOO_VAULT` env var writes vault recipe. Direct env values as fallback for dev/local.

6. **Parallel sessions from day one** — actor model with shared tool executor, not sequential queue. Decided against process-pool (too heavy) and shared-bus-mutex (contention during tool exec).

## Multi-Session Router — Implementation Spec

### Architecture: Actor Model

```
                         ┌─────────────────────────────────┐
Discord msg ──→ vox ──→ │ Session Router                   │
                         │                                   │
                         │  SessionKey → Actor task          │
                         │                                   │
                         │  "discord:U123" ──→ Actor A ──┐  │
                         │  "discord:U456" ──→ Actor B ──┼──→ LLM (concurrent)
                         │  "slack:U789"   ──→ Actor C ──┘  │
                         │                         │         │
                         │                         ▼         │
                         │               Tool Executor       │
                         │               (owns EventBus)     │
                         │               (serialized exec)   │
                         └─────────────────────────────────┘
```

### Key Structs

```rust
/// Manages all active sessions. Lives in run_embedded_command.
struct SessionRouter {
    sessions: HashMap<String, SessionHandle>,
    tool_tx: mpsc::Sender<ToolRequest>,
    bridge: Arc<dyn LlmBridge>,
    shared_settings: Arc<Mutex<Settings>>,
    events_tx: broadcast::Sender<AgentEvent>,
    max_sessions: usize,           // e.g., 100
    session_timeout: Duration,      // e.g., 30 min
}

/// Handle to a running session actor.
struct SessionHandle {
    tx: mpsc::Sender<SessionMessage>,  // send messages to this session
    last_active: Instant,
    session_key: String,
    trust_level: String,               // for logging/monitoring
    task: JoinHandle<()>,
}

/// Messages sent to session actors.
enum SessionMessage {
    Inbound {
        text: String,
        trust_level: String,
        reply_context: Value,
    },
    Shutdown,
}

/// Requests from session actors to the tool executor.
struct ToolRequest {
    session_id: String,
    method: ToolMethod,
    response_tx: oneshot::Sender<ToolResponse>,
}

enum ToolMethod {
    BuildSystemPrompt { ... },
    ExecuteTool { name: String, args: Value },
    ProcessEvents { ... },
    GetTools,
}
```

### Session Actor Lifecycle

```
1. First message for unknown SessionKey
   → Router creates new SessionHandle
   → Spawns tokio task with own Conversation + ContextManager
   → Task enters recv loop, waiting for SessionMessage

2. Message arrives for existing session
   → Router sends SessionMessage::Inbound via channel
   → Actor pushes to conversation, calls LLM (directly, no lock)
   → LLM responds with tool calls → actor sends ToolRequest to executor
   → Executor runs tool, sends response back via oneshot
   → Actor continues LLM loop

3. Idle timeout (30 min no messages)
   → Router cleanup tick evicts session
   → Sends SessionMessage::Shutdown
   → Actor persists conversation to disk, exits

4. Session limit reached
   → Oldest idle session evicted to make room
```

### Tool Executor

Single tokio task, owns the EventBus exclusively:

```rust
async fn tool_executor(
    mut bus: EventBus,
    mut rx: mpsc::Receiver<ToolRequest>,
) {
    while let Some(req) = rx.recv().await {
        let result = match req.method {
            ToolMethod::ExecuteTool { name, args } => {
                bus.execute_tool(&name, &args).await
            }
            ToolMethod::BuildSystemPrompt { .. } => {
                bus.build_system_prompt(...)
            }
            ToolMethod::GetTools => {
                bus.tools()
            }
            ...
        };
        let _ = req.response_tx.send(result);
    }
}
```

No `Arc<Mutex<>>` needed — the executor owns the bus outright. Session actors communicate via channels. The LLM call happens in the actor, not the executor — so N sessions can have N concurrent LLM round-trips.

### What Changes in Existing Code

**New files in omegon:**
- `core/crates/omegon/src/session_router.rs` — SessionRouter, SessionHandle, actor loop
- `core/crates/omegon/src/tool_executor.rs` — ToolExecutor, request/response types

**Modified files:**
- `main.rs` (`run_embedded_command`) — when vox_polling_handles is non-empty, create SessionRouter + ToolExecutor instead of direct dispatch loop. Single-session path (no vox) stays unchanged.
- `r#loop.rs` — needs a variant or trait that dispatches tool calls via channel instead of direct `&mut bus`. OR: the session actor reimplements the loop using the executor channel directly (simpler, avoids changing the shared loop code).

**Unchanged:**
- `r#loop::run` — TUI and headless paths continue to use `&mut EventBus` directly
- All features, tools, extensions — no changes
- vox, vox-core, vox-discord, vox-slack — no changes

### Profile-Driven Activation

The session router activates based on runtime state, not config:

```rust
// In run_embedded_command, after extension discovery:
if !vox_polling_handles.is_empty() {
    // External users detected → parallel multi-session mode
    let (tool_tx, tool_rx) = mpsc::channel(64);
    let executor_task = tokio::spawn(tool_executor(agent.bus, tool_rx));
    let mut router = SessionRouter::new(tool_tx, bridge, ...);
    
    // Dispatch loop routes to sessions
    loop {
        select! {
            _ = vox_poll.tick() => {
                for envelope in drain_events() {
                    let session_key = extract_session_key(&envelope);
                    router.route(session_key, envelope).await;
                }
            }
            _ = cleanup_tick.tick() => {
                router.evict_idle_sessions().await;
            }
        }
    }
} else {
    // No external users → single-session mode (current behavior)
    loop { ... }
}
```

### Concurrency Characteristics

| Operation | Lock/Channel | Duration | Concurrent? |
|-----------|-------------|----------|-------------|
| LLM API call | None | 5-30s | Yes, fully |
| System prompt build | Tool executor channel | ~1ms | Serialized, fast |
| Tool execution | Tool executor channel | 1ms-10s | Serialized |
| Conversation push | Per-session (no sharing) | ~0µs | Yes |
| Memory read/write | SQLite (internal locking) | ~1ms | Serialized by SQLite |

In practice: 10 concurrent sessions = 10 concurrent LLM calls. Tool execution serializes briefly through the executor. The bottleneck is LLM API rate limits, not internal contention.

### Session Persistence

Sessions that survive container restarts:

```
$OMEGON_HOME/sessions/
  discord-U123.jsonl        # conversation log
  discord-U456.jsonl
  slack-U789-T100.jsonl
```

On eviction: persist conversation to JSONL. On resume: load and replay into new Conversation. This is the same pattern as omegon's existing session resume for interactive mode.

### Implementation Order

1. **ToolExecutor** — channel-based bus proxy. Start here because it's self-contained and testable.
2. **SessionRouter** — session map, create/route/evict lifecycle.
3. **Session actor** — conversation loop using executor channel for tool dispatch.
4. **Dispatch loop rewrite** — `run_embedded_command` branches on vox presence.
5. **Session persistence** — JSONL save/load for idle eviction.
6. **Cleanup** — idle timeout tick, max session cap, graceful shutdown.

### Estimated Scope

- ~4 new files (session_router.rs, tool_executor.rs, session_actor.rs, session_persist.rs)
- ~1 modified file (main.rs dispatch loop)
- ~0 changes to existing loop, bus, features, tools, or extensions
- Existing single-session paths (TUI, headless, cleave) completely unaffected

## Critical Context

- The `r#loop::run` function is the heart of omegon — do NOT modify it for this. The session actor should reimplement the turn loop using the executor channel. This keeps the change additive rather than invasive.

- `EventBus` takes `&mut self` for `on_event` and tool execution. The tool executor pattern avoids the need for `Arc<Mutex<>>` by giving the executor exclusive ownership.

- The `bridge` (`Box<dyn LlmBridge>`) needs to become `Arc<dyn LlmBridge>` for sharing across session actors. LlmBridge is already `Send + Sync` (async trait with `&self` methods).

- omegon already has the cleave task file for this: `.cleave-implement-daemon-session-router-in-omego/`. Review it for any prior planning context.

- Trust level enforcement happens before routing — the `format_vox_event` containment wrapping is already applied by the bridge before the message reaches the router. The router doesn't need to know about trust levels.

## File Reference

**Implementation targets (omegon):**
- `core/crates/omegon/src/main.rs:922-985` — current vox dispatch loop (replace with router)
- `core/crates/omegon/src/r#loop.rs` — reference for turn loop logic (do not modify)
- `core/crates/omegon/src/extensions/vox_bridge.rs` — bridge that feeds the router
- `.cleave-implement-daemon-session-router-in-omego/` — prior planning context

**Vox (no changes needed):**
- `vox-core/src/lib.rs:432-468` — SessionKey derivation
- `vox/src/main.rs:312-327` — execute_route includes session_key in output

**Config (no changes needed):**
- `deploy/entrypoint.sh` — env var config gen + vault recipes
- `deploy/discord-agent.toml` — access control config
