+++
id = "ddf369bd-c863-4635-8037-e88f882654e8"
kind = "document"
title = "MCP transport for plugin tools — Model Context Protocol as first-class tool source"
status = "implemented"
tags = ["architecture", "plugins", "mcp", "tools", "interoperability", "standards"]
aliases = ["mcp-transport"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
issue_type = "feature"
open_questions = []
priority = "1"
+++

# MCP transport for plugin tools — Model Context Protocol as first-class tool source

## Overview

Support MCP (Model Context Protocol) servers as a tool source alongside the existing HTTP, script, and OCI runners. MCP is the emerging industry standard — every MCP server in the ecosystem becomes an Omegon plugin with zero adaptation. This is the single highest-impact gap vs OpenCode.

## Research

### Rust MCP ecosystem — official SDK and crate options

**Official SDK**: `modelcontextprotocol/rust-sdk` → crate `rmcp`
- Official MCP org repo. Active development.
- Transports: stdio (`transport-io`), child-process spawn (`transport-child-process`), streamable HTTP client/server
- Full protocol: tools, resources, prompts, logging, sampling
- Client and server roles — we need client (connect to external MCP servers)
- Uses tokio async, reqwest for HTTP

**Community SDK**: `mcp-protocol-sdk` (crates.io v0.5.0)
- Claims 100% schema compliance with MCP 2025-06-18
- stdio, HTTP, WebSocket transports
- Feature-gated: `stdio`, `http`, `websocket`, `validation`

**Recommendation**: Use the official `rmcp` crate. Maintained by MCP org, matches protocol exactly, has client-side child-process transport (spawn MCP server as subprocess, talk via stdio — the standard pattern).

**Integration point**: Add `runner = "mcp"` to plugin.toml alongside script/http/oci/wasm. MCP servers declared as:
```toml
[mcp_servers.filesystem]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/path"]

[mcp_servers.brave-search]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-brave-search"]
env = { BRAVE_API_KEY = "{BRAVE_API_KEY}" }
```
Tools from MCP servers register alongside native tools in the tool registry. The agent sees a flat tool list regardless of source.

### MCP over Styrene mesh — PQC-encrypted remote tool execution via RNS/Yggdrasil

Styrene provides a mesh communications platform with:
- **styrene-rns**: Reticulum Network Stack protocol core (identity, packets, crypto, ratchets)
- **styrene-mesh**: Wire protocol envelope (msgpack over RNS/Yggdrasil)
- **styrene-tunnel**: PQC tunnels (ML-KEM-768 + X25519 hybrid key exchange, AES-256-GCM)
- **styrene-ipc**: Daemon IPC contract with `DaemonFleet` trait — `exec()`, `terminal_open/input/close()`
- **styrene-content**: P2P content distribution (chunk-based, works on RP2040 bare-metal)

The key insight: `DaemonFleet::exec()` already runs commands on remote devices over the mesh. An MCP server running on a remote node is just a process — `exec()` spawns it, and the mesh carries the stdio JSON-RPC traffic.

**This is a fourth MCP transport mode:**
1. Local process (stdio)
2. OCI container (podman/docker stdio)
3. Docker MCP Gateway
4. **Styrene mesh** (remote exec over RNS/Yggdrasil, PQC-encrypted)

The config would look like:
```toml
[mcp_servers.gpu-cluster]
styrene_dest = "a7b3c9d1..."  # RNS destination hash
command = "/opt/mcp-servers/gpu-inference"
# Traffic is PQC-encrypted via styrene-tunnel
# No TCP, no HTTP, no TLS — mesh transport handles everything
```

This means an operator with Omegon on their laptop could call tools on:
- A GPU workstation in their home lab (KiCad DRC, heavy inference)
- A Raspberry Pi running sensor MCP servers
- A cloud instance accessible only over the Styrene mesh (no public IP)
- Another developer's machine sharing a specific MCP server

All encrypted end-to-end with post-quantum cryptography, no ports to open, no VPN to manage.

The implementation path: write a `StyreneMcpTransport` that implements rmcp's `Transport` trait by wrapping `DaemonFleet::exec()` for command spawn + `terminal_open/input` for bidirectional stdio. The rmcp protocol layer stays identical — only the transport changes.

## Decisions

### Decision: Use official rmcp crate (v1.2) with child-process transport for MCP server connections

**Status:** decided
**Rationale:** The official modelcontextprotocol/rust-sdk crate (rmcp) is stable at v1.2, supports child-process spawning of MCP servers via TokioChildProcess, and provides the full MCP protocol (tools, resources, prompts). McpFeature implements the Omegon Feature trait — tools from MCP servers register alongside native tools in a flat list. Server names prefix tool names to avoid collisions. ArmoryManifest gains mcp_servers section so plugins can declare MCP servers alongside script/HTTP/OCI tools.

### Decision: Three MCP server execution modes: local process, OCI container, Docker MCP Gateway

**Status:** decided
**Rationale:** Local process is the standard MCP pattern (npx, uvx, etc). OCI containers run MCP servers in podman/docker with mount_cwd and network policy — podman preferred (rootless, daemonless), docker fallback, OMEGON_CONTAINER_RUNTIME env override. Docker MCP Gateway integrates with Docker Desktop MCP Toolkit (200+ pre-configured servers). All three share the same stdio JSON-RPC transport — the agent sees a flat tool list regardless of execution mode.

### Decision: Styrene MCP transport uses terminal_open/input for bidirectional streaming over the mesh

**Status:** decided
**Rationale:** MCP JSON-RPC is a persistent bidirectional stream — each tool call is a request/response within an ongoing session, not a one-shot command. DaemonFleet::terminal_open() opens a persistent session to the remote device, terminal_input() sends data (JSON-RPC requests), and the daemon's event stream delivers output (JSON-RPC responses). This maps directly to rmcp's Transport trait (AsyncRead + AsyncWrite). DaemonFleet::exec() is reserved for one-shot operations like checking if the MCP server binary exists on the remote node.

## Open Questions

*No open questions.*
