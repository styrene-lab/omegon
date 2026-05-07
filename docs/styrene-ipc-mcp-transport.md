+++
id = "859a696f-6236-4e8c-b7fe-3deaa20f8859"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Native styrene-ipc MCP transport — zero-overhead mesh tool execution via DaemonFleet

## Overview

Replace the current `styrene exec` CLI bridge with native styrene-ipc integration. Implement rmcp's Transport trait by wrapping DaemonFleet::terminal_open() for bidirectional stdio over the mesh. This eliminates the CLI subprocess overhead and gives Omegon direct access to the Styrene daemon's connection management, session lifecycle, and PQC tunnel state.

## Research

### Implementation path: DaemonFleet → rmcp Transport trait

The rmcp `Transport` trait needs `AsyncRead + AsyncWrite`. The Styrene daemon provides:

- `DaemonFleet::terminal_open(request)` → `SessionId` — opens a bidirectional terminal session to a remote node
- `DaemonFleet::terminal_input(session_id, data)` — sends bytes to the remote process stdin
- `DaemonEvents` — delivers output bytes from the remote process stdout via broadcast channels

The bridge implementation:
1. `StyreneMcpTransport` wraps a `Daemon` instance + `SessionId`
2. On construction: calls `terminal_open()` with the MCP server command
3. `AsyncWrite::write()` → `terminal_input(session_id, data)`
4. `AsyncRead::read()` → subscribes to daemon events, buffers terminal output
5. On drop: calls `terminal_close(session_id)`

This is a clean adapter pattern. The rmcp protocol layer is unchanged — only the transport bytes-on-the-wire are now mesh-routed instead of local stdio.

Dependencies: styrene-ipc crate needs to be added to the omegon workspace. Feature-gated: `omegon --features=styrene` to avoid pulling in the Styrene dependency stack for operators who don't use it.

## Open Questions

- Now that rmcp Streamable HTTP transport is enabled, does native styrene-ipc still add value over routing mesh MCP servers through the HTTP transport with Styrene tunnel as the network layer?
