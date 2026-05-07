+++
id = "adb6a7fc-48a9-4216-88b2-869d6784c14d"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Extension Lifecycle Management Design

## Overview

Extensions can be enabled, disabled, and reloaded from the TUI without restarting Omegon. Extension state is persistent across sessions.

## Current State

Today, extensions are:
- Discovered at TUI startup from `~/.omegon/extensions/`
- Validated (manifest, SDK version, health check)
- Spawned as processes
- Stay alive for the entire TUI session
- No way to toggle or reload them

## Desired Behavior

### User Perspective

```
/extensions                    # Opens extension management panel

Installed Extensions (4):
  ✓ scribe-rpc (v0.2.0)        [active]   [disable] [reload]
  ✓ python-analyzer (v0.1.0)   [active]   [disable] [reload]
  ✗ my-broken-ext (v0.0.1)     [disabled] [enable]  [delete]

Inactive Extensions (1):
  • custom-fork (local)        [enable]   [reload]
```

### Actions

**Enable**
- Click [enable] on disabled extension
- Omegon validates and spawns process
- Health check succeeds → extension ready
- Widgets registered, tools available
- Extension mind loaded (if BYOM)

**Disable**
- Click [disable] on active extension
- Omegon sends SIGTERM to extension process
- Waits 5 seconds for graceful shutdown
- If still alive, sends SIGKILL
- Widgets removed from UI
- Tools no longer available
- Extension mind unloaded (but persisted to disk)

**Reload**
- Useful during development
- Disables the extension (as above)
- Rebuilds binary if needed (detect Cargo.toml changes)
- Re-enables (as above)
- Hot-reload without TUI restart

**Delete**
- Removes extension from `~/.omegon/extensions/{name}/`
- Includes mind data if BYOM
- Confirmation required

### Extension State

Each extension has state persisted to `~/.omegon/extensions/{name}/.omegon/state.toml`:

```toml
[extension]
enabled = true                    # Whether to spawn on TUI start
last_enabled_at = "2024-03-31T14:00:00Z"
last_disabled_at = "2024-03-31T13:00:00Z"

[stability]
crashes = 0                       # Number of crashes this session
health_check_failures = 0         # Failed health checks
last_error = ""                   # Last error message
```

## Architecture

### Extension Process Lifecycle

```
[TUI startup]
  ↓
  For each extension in ~/.omegon/extensions/:
    1. Load state.toml
    2. If state.enabled = true:
       - Validate manifest
       - Spawn process
       - Health check
       - On success: mark active
       - On failure: mark disabled, log error
    3. If state.enabled = false:
       - Skip spawning
       - Mark inactive
  ↓
[TUI running]
  ↓
  User clicks [enable] on extension
    1. Validate manifest
    2. Spawn process
    3. Health check
    4. Load mind data (if BYOM)
    5. Update state.toml (enabled = true)
  ↓
  User clicks [disable] on extension
    1. Unload mind data (save to disk)
    2. SIGTERM to extension process
    3. Wait 5 seconds
    4. SIGKILL if still alive
    5. Update state.toml (enabled = false)
  ↓
  User clicks [reload] on extension
    1. Detect if Cargo.toml exists
    2. If yes, run: cargo build --release
    3. Disable extension (SIGTERM + SIGKILL)
    4. Enable extension (spawn + health check)
    5. Update state.toml
```

## Extension Stability Monitoring

Track crashes and failures:

```rust
#[derive(Serialize, Deserialize)]
pub struct ExtensionState {
    pub enabled: bool,
    pub last_enabled_at: Option<String>,
    pub last_disabled_at: Option<String>,
    pub stability: StabilityMetrics,
}

#[derive(Serialize, Deserialize)]
pub struct StabilityMetrics {
    pub crashes_this_session: u32,
    pub health_check_failures: u32,
    pub last_error: Option<String>,
    pub last_error_at: Option<String>,
}
```

If an extension crashes more than 3 times in one TUI session:
- Auto-disable it
- Log error: "Extension {name} crashed 3 times, auto-disabled"
- User can manually re-enable later

## RPC Protocol Extensions

New RPC methods for lifecycle management:

### `shutdown` (extension → parent)

Extension can request graceful shutdown:

```json
{"jsonrpc": "2.0", "method": "shutdown", "params": {"reason": "fatal error"}}
```

Omegon treats this as intentional shutdown, doesn't count as crash.

### `set_enabled_state` (parent → extension)

Notify extension when it's about to be disabled:

```json
{"jsonrpc": "2.0", "id": "1", "method": "set_enabled_state", "params": {"enabled": false}}
```

Extension can clean up resources before shutdown.

### `get_stability` (parent → extension)

Query extension stability metrics:

```json
{"jsonrpc": "2.0", "id": "1", "method": "get_stability", "params": {}}
```

Response:

```json
{
  "jsonrpc": "2.0",
  "id": "1",
  "result": {
    "healthy": true,
    "uptime_seconds": 3600,
    "memory_bytes": 52428800,
    "errors": []
  }
}
```

## Manifest Changes

Optional new fields in manifest.toml:

```toml
[extension]
name = "scribe-rpc"
version = "0.2.0"

[lifecycle]
# Should extension be enabled by default on first install?
enabled_by_default = true

# Can extension be safely disabled without losing state?
stateless = false               # If true, disabling has no side effects

# Auto-reload manifest changes?
auto_reload_manifest = true     # Detect manifest.toml changes, reload

[stability]
# Auto-disable if crashes exceed this threshold per session
crash_threshold = 3

# Time window for crash counting (seconds)
crash_window = 3600             # 1 hour

# Should extension be disabled if health check fails at startup?
fail_startup_health_check = true
```

## TUI Panel Design

```
/extensions management panel:

┌─ Extensions Management ────────────────────────────────────────┐
│                                                               │
│ ACTIVE (3):                                                   │
│ ┌─────────────────────────────────────────────────────────────┐
│ │ scribe-rpc v0.2.0 (registry) [hl: 3 hours] [⬆ update]     │
│ │ python-analyzer v0.1.0 (git:user/repo) [⬆ update]         │
│ │ my-extension v0.0.1 (local)                               │
│ │                                                             │
│ │ Press 'd' to disable, 'r' to reload, 'i' for info         │
│ └─────────────────────────────────────────────────────────────┘
│                                                               │
│ INACTIVE (1):                                                 │
│ ┌─────────────────────────────────────────────────────────────┐
│ │ broken-extension v0.0.1 (local)                            │
│ │ Status: crashed 3 times, auto-disabled                    │
│ │ Last error: panic in handle_rpc                           │
│ │                                                             │
│ │ Press 'e' to enable, 'd' to delete                        │
│ └─────────────────────────────────────────────────────────────┘
│                                                               │
│ [Search Extensions] [Browse Registry] [Install New]          │
└─────────────────────────────────────────────────────────────┘
```

## Implementation Order

1. **Phase 1:** Basic enable/disable (spawn/SIGTERM)
2. **Phase 2:** Stability tracking (crash counting, auto-disable)
3. **Phase 3:** TUI panel for management
4. **Phase 4:** Reload functionality
5. **Phase 5:** BYOM integration (separate design node)

## Backward Compatibility

- Extensions don't need to implement shutdown/set_enabled_state
- If not implemented, Omegon handles cleanly (just SIGTERM/SIGKILL)
- Old extensions without lifecycle awareness still work fine
