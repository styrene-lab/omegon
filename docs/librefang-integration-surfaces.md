+++
id = "90d9591f-1853-49c5-baac-f199496c956a"
kind = "document"
title = "Librefang Integration Surfaces"
status = "planned"
tags = ["librefang", "integration", "provider", "mcp", "a2a", "armory"]
aliases = ["librefang-integration-surfaces"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
issue_type = "architecture"
related = ["a2a-protocol-integration", "armory-discovery", "mcp-transport", "bridge-provider-routing", "provider-credential-map"]
open_questions = [
  "What default Librefang daemon URL and auth mechanism should Omegon assume, if any?",
  "Should Librefang be configured as a generic OpenAI-compatible provider or as a named provider profile with dedicated health checks?",
  "Should the A2A/OFP bridge live in Omegon directly or in Auspex/operator first?",
  "What Armory package shape best represents Librefang: provider, MCP peer, A2A bridge, or bundle?"
]
+++

# Librefang Integration Surfaces

## Overview

Librefang is best treated as an external peer runtime that Omegon can discover, route through, and federate with. It should not be vendored into Omegon core. The upstream project overlaps heavily with Omegon's own runtime, provider, MCP, ACP, memory, skills, telemetry, and desktop surfaces, so the integration should start at protocol and packaging boundaries rather than crate-level reuse.

The practical goal is to make a running Librefang instance easy to use from Omegon without changing Omegon's default loop:

- route model calls through Librefang's OpenAI-compatible API when the operator opts in;
- expose Librefang as an Armory-discoverable ecosystem package;
- provide MCP configuration templates where Librefang is acting as an MCP peer;
- reserve A2A/OFP federation for a later bridge, ideally through Auspex or an Armory extension.

## Upstream Surface Inventory

The current upstream repository exposes several surfaces that are relevant to Omegon:

- `crates/librefang-api/src/server.rs` provides OpenAI-compatible `/v1/chat/completions` and `/v1/models` routes.
- `crates/librefang-runtime-mcp/src/lib.rs` provides MCP client/runtime behavior, including policy and taint-scanning concepts worth studying.
- `crates/librefang-wire/src/lib.rs` defines the OFP wire protocol with HMAC and Ed25519 trust-on-first-use identity. Upstream notes that confidentiality is provided by an external network layer such as WireGuard, Tailscale, SSH, or a service mesh.
- `crates/librefang-acp/src/lib.rs` adapts Librefang to Agent Client Protocol, but it currently tracks a newer ACP crate than Omegon.
- `crates/librefang-hands/src/lib.rs` defines `HAND.toml` capability-package metadata that maps conceptually to Armory agents, skills, and extensions.
- `crates/librefang-runtime-wasm/src/lib.rs` sketches a WASM skill sandbox.
- `crates/librefang-llm-driver` and `crates/librefang-llm-drivers` provide typed provider driver and failover concepts that are useful as design references, but not as direct dependencies.

The upstream project is broad and beta-versioned. That makes protocol-level integration safer than trying to share runtime internals.

## Integration Principle

Librefang should be integrated as a peer, not as a library. Omegon should depend on stable external contracts:

- HTTP provider API;
- MCP transport;
- Armory metadata and install instructions;
- optional future A2A/OFP bridge behind a security boundary.

Omegon should avoid pulling in Librefang's workspace dependencies directly. A direct dependency would introduce large blast radius from `wasmtime`, OpenTelemetry, Tauri/desktop, `rmcp`, and ACP version drift while duplicating runtime responsibilities Omegon already owns.

## Surface 1: Named OpenAI-Compatible Provider

Librefang's lowest-risk integration is a named provider profile backed by Omegon's existing OpenAI-compatible provider path.

Operator-facing shape:

```sh
omegon provider add librefang --base-url http://127.0.0.1:<port>/v1
omegon provider check librefang
omegon --provider librefang --model <librefang-model-or-agent>
```

The first implementation should require an operator-supplied `base_url` until the Librefang daemon's default port and auth behavior are confirmed from upstream configuration or release docs. If Librefang requires auth, the provider profile should resolve an optional `LIBREFANG_API_KEY` or Omegon-managed secret without exposing that value to the agent transcript.

Provider health should call `/v1/models` and surface:

- daemon reachable;
- model list returned;
- auth required or missing;
- selected model present;
- OpenAI-compatible chat route available.

This should not introduce a new model-routing subsystem. It is a named profile over an already-supported protocol.

## Surface 2: Armory Package

Librefang should become discoverable through Armory before it becomes deeply integrated into core. The first package can be an ecosystem package rather than a native bundled extension.

Candidate package names:

- `styrene.librefang-provider`
- `styrene.librefang-peer`
- `styrene.librefang-bridge`

Initial package metadata should include:

- upstream repository and license;
- tested upstream commit or release;
- install and run instructions;
- provider configuration snippet for Omegon;
- health-check command;
- capabilities: `provider`, `openai-compatible`, `mcp-peer`, `a2a-candidate`;
- security notes about local-only binding, auth, and network exposure.

If Armory does not yet have a first-class `provider` package kind, represent this as an extension or plugin bundle with provider instructions. Do not block the package on schema work unless the UI cannot present it clearly.

## Surface 3: MCP Peer Configuration

Librefang's MCP runtime should be integrated through configuration templates, not code reuse.

The first useful artifact is an Armory-provided MCP template that tells an operator how to attach Librefang as either:

- a stdio MCP server, if upstream exposes one;
- a streamable HTTP MCP endpoint, if upstream exposes one;
- a local sidecar process controlled outside Omegon.

Omegon should namespace Librefang tools clearly so they do not collide with native tools or other MCP servers. The MCP template should also mark Librefang-originated tools as external and subject to the same policy and consent model as any other MCP tool.

Librefang's taint-policy work is worth studying for Omegon, but that should be a separate hardening track. Do not import it as part of the initial peer configuration.

## Surface 4: A2A/OFP Federation Bridge

Librefang's OFP wire protocol is a federation candidate, but it should not become an Omegon control-plane default. Its current security posture relies on external transport confidentiality, while Omegon and Auspex have been moving toward WSS/mTLS as a control-plane invariant.

The bridge should therefore be experimental and explicit:

- package it as an Armory extension or Auspex-managed sidecar;
- run only on loopback, mTLS, or a trusted overlay network;
- map Omegon/Auspex tasks to Librefang peer messages;
- expose capability discovery separately from task execution;
- log cross-runtime delegation as a first-class audit event.

Auspex is likely the better first home for federation because it already owns multi-agent orchestration, workflow definition, and operator-facing topology. Omegon should remain capable of using the bridge locally, but should not absorb federation policy into the core loop prematurely.

## Surface 5: ACP Interop Testing

Both projects have ACP surfaces, but upstream Librefang currently tracks a newer ACP crate than Omegon. This is an interop-test target, not a reason to bump Omegon's ACP dependency by itself.

Useful tests:

- run Librefang behind its ACP adapter and verify basic session creation;
- verify prompt send, progress, and terminal/file reverse RPC behavior;
- compare permission semantics with Omegon's ACP methods;
- record version mismatch issues before changing dependencies.

ACP should be used to validate editor and client compatibility. It should not be the first transport for model routing because the OpenAI-compatible endpoint is simpler and lower risk.

## Surface 6: Hands to Armory Mapping

Librefang's `HAND.toml` format overlaps with Armory's package descriptors. The right integration is a converter or import path, not a wholesale schema replacement.

Potential mapping:

| Librefang concept | Armory/Omegon concept |
| --- | --- |
| Hand metadata | Armory package metadata |
| Hand requirements | install checks / prerequisites |
| Hand settings | extension or provider config |
| Hand lifecycle | install, enable, health, uninstall |
| Hand dashboard metrics | Armory package status / Auspex observability |

This should wait until the basic provider package exists. Once there are real examples, decide whether a `hand-to-armory` importer is worth building.

## Surface 7: Telemetry and Status

Omegon should initially treat Librefang telemetry as peer status, not as internal loop telemetry.

Minimal status fields:

- provider reachable;
- model count;
- selected model present;
- MCP peer connected;
- bridge connected, if enabled;
- last health-check failure;
- configured security mode: loopback, token, mTLS, overlay, or unknown.

Auspex can later consume these fields to render Librefang as an external runtime node in a workflow graph.

## Phasing

### Phase 0: Document and Track

- Add this integration-surface plan.
- Create or update an Armory issue/package plan for a Librefang provider package.
- Confirm upstream daemon run command, default URL, and auth behavior before hardcoding defaults.

### Phase 1: Provider Profile

- Add a named `librefang` provider profile over the existing OpenAI-compatible provider client.
- Add `/v1/models` health probing.
- Add docs for operator-supplied `base_url` and optional secret-backed auth.
- Validate one prompt round trip against a locally running Librefang instance.

### Phase 2: Armory Discovery

- Publish a Librefang package entry with source links, install notes, health checks, and provider config.
- Make the package visible in Armory filters under provider/external-runtime capability tags.
- Avoid auto-enabling until an operator explicitly opts in.

### Phase 3: MCP Template

- Add a Librefang MCP configuration recipe once the upstream MCP serving mode is confirmed.
- Validate tool discovery and namespacing.
- Ensure tool calls preserve Omegon's existing external-tool consent and audit behavior.

### Phase 4: Federation Bridge

- Prototype an OFP/A2A bridge as an Armory extension or Auspex-managed sidecar.
- Require loopback, mTLS, or trusted-overlay deployment.
- Map task lifecycle and audit events before allowing general delegation.

### Phase 5: Optional Schema Import

- Evaluate `HAND.toml` to Armory metadata conversion after at least one real package exists.
- Keep Armory as the operator-facing catalog source of truth.

## Non-Goals

- Do not vendor Librefang into Omegon core.
- Do not adopt OFP as Omegon's control-plane protocol.
- Do not bypass Omegon's provider, MCP, secrets, consent, or audit boundaries.
- Do not bump ACP or MCP dependencies solely for Librefang unless an interop test demonstrates the need.
- Do not merge Librefang memory, runtime, desktop, or telemetry internals into Omegon.

## Blast Radius

| Surface | Risk | Reason |
| --- | --- | --- |
| Named OpenAI-compatible provider | Low | Uses existing provider abstraction and a stable HTTP shape. |
| Armory package | Low | Metadata and docs only; operator opt-in. |
| MCP template | Medium | Exposes external tools, but within existing MCP policy surfaces. |
| ACP interop testing | Medium | Version drift may reveal dependency pressure. |
| OFP/A2A bridge | High | Cross-runtime delegation and transport security need explicit policy. |
| Direct crate dependency | Very high | Large overlapping workspace, dependency churn, ACP/MCP version drift, and runtime duplication. |

## Acceptance Checks

The first useful integration is complete when:

- an operator can configure a Librefang `base_url` without editing source;
- `omegon provider check librefang` can verify `/v1/models`;
- Omegon can send one chat completion through Librefang by explicit provider selection;
- Armory can show Librefang with source links, install notes, and capability tags;
- no default Omegon routing changes when Librefang is absent;
- secrets used for Librefang auth do not enter the agent-visible transcript.
