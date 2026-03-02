# mcp-bridge

Pi extension that connects to MCP servers and registers their tools as native pi tools. Supports both **stdio** (local process) and **Streamable HTTP** (remote server) transports.

## Configuration

Create `mcp.json` in this extension directory. Each entry in `servers` is either a stdio or HTTP server, discriminated by the presence of `url` vs `command`.

### Streamable HTTP server

```json
{
  "servers": {
    "scribe": {
      "url": "https://scribe.example.com/mcp/transport/",
      "headers": {
        "Authorization": "Bearer ${GITHUB_TOKEN}"
      },
      "timeout": 15000
    }
  }
}
```

| Field | Required | Description |
|-------|----------|-------------|
| `url` | yes | MCP Streamable HTTP endpoint URL (use canonical URL — no trailing-slash redirects) |
| `headers` | no | HTTP headers; supports `${ENV_VAR}` interpolation |
| `timeout` | no | Connection timeout in ms (default: 15000) |

### Stdio server

```json
{
  "servers": {
    "my-tool": {
      "command": "npx",
      "args": ["-y", "@example/mcp-server"],
      "env": {
        "API_KEY": "${MY_API_KEY}"
      }
    }
  }
}
```

| Field | Required | Description |
|-------|----------|-------------|
| `command` | yes | Executable to spawn |
| `args` | no | Command arguments |
| `env` | no | Environment variables; supports `${ENV_VAR}` interpolation |

## Secret management

Environment variables in `${...}` are resolved via `process.env` at connect time. Use the **00-secrets** extension to populate secrets from keychains, CLI tools, or other backends.

This extension declares `GITHUB_TOKEN` via the `@secret` annotation. To configure:

```
/secrets configure GITHUB_TOKEN
```

Or add a recipe to `~/.pi/agent/secrets.json`:

```json
{
  "GITHUB_TOKEN": "!gh auth token"
}
```

## Behavior

- **Parallel connection**: All servers connect concurrently at session start. One slow/failing server does not block others.
- **Timeouts**: Each connection has an independent timeout (default 15s). Failures are reported per-server.
- **Reconnection**: On transport-level errors (connection reset, fetch failure), the bridge attempts one automatic reconnect + retry before returning an error.
- **Tool naming**: Tools are registered as `mcp_{server}_{tool}`, e.g., `mcp_scribe_list_partnerships`.
- **Shutdown**: All connections are closed on session shutdown.

## Commands

| Command | Description |
|---------|-------------|
| `/mcp` | List connected servers, transport type, and registered tools |
