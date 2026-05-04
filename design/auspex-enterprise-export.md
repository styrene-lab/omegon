# Auspex Enterprise Export — MQTT to SIEM / Kafka / OTLP

Status: exploring
Related: styrene-mqtt, auspex-telemetry-aggregation, aether

## Overview

Auspex becomes the export adapter from the local MQTT event fabric to enterprise observability infrastructure. Omegon, Scry, Viz, and other services publish events to the MQTT broker without knowledge of downstream consumers. Auspex subscribes, applies operator-defined filtering and redaction, and pushes to external systems.

## Architecture

Auspex owns the MQTT broker. Omegon instances, Scry, Viz, and Aether connect
as TCP clients. Auspex holds an in-process link for its own aggregation pipeline.

```
                    ┌──────────────────────────────────┐
                    │        Auspex (operator)          │
                    │                                   │
                    │  ┌─────────────────────────────┐  │
                    │  │  Embedded MQTT Broker        │  │
                    │  │  (rumqttd, in-process link)  │  │
                    │  │  TCP :1883 for clients       │  │
                    │  └──────────────┬──────────────┘  │
                    │                 │                  │
                    │  ┌──────────────▼──────────────┐  │
                    │  │  Aggregator / Redactor       │  │
                    │  │  (in-process subscriber)     │  │
                    │  └──────────────┬──────────────┘  │
                    │                 │                  │
                    │  ┌──────────────▼──────────────┐  │
                    │  │  Export Router               │  │
                    │  └──────┬───────┬───────┬──────┘  │
                    └─────────┼───────┼───────┼─────────┘
                              │       │       │
                      ┌───────▼┐ ┌────▼──┐ ┌──▼─────┐
                      │ Kafka  │ │ OTLP  │ │ Syslog │
                      │ (ES)   │ │(DD/NR)│ │ (SIEM) │
                      └────────┘ └───────┘ └────────┘
                    
         TCP :1883 clients:
         ┌──────────┐  ┌──────────┐  ┌────────┐  ┌────────┐
         │ Omegon A │  │ Omegon B │  │  Scry  │  │  Viz   │
         │ (publish) │  │ (publish) │  │(publish)│ │ (sub)  │
         └──────────┘  └──────────┘  └────────┘  └────────┘
```

## Three Roles

| Component | Role | MQTT relationship |
|-----------|------|-------------------|
| **Aether** | Collects — subscribes to `#`, aggregates local node state, bridges to mesh | Subscriber + publisher |
| **Auspex** | Exports — subscribes with operator filters, redacts, pushes to enterprise backends | Subscriber only |
| **styrene-mqtt** | Transport — broker (embedded in Auspex), client lib, topic routing, QoS delivery | Infrastructure |

## Two Communication Planes

**Plane 1: Styrene Mesh (LXMF over RNS)** — agent-to-agent across nodes. Store-and-forward, Ed25519 signing, PQC tunnels, tier hierarchy, delegation chains. Works over LoRa/packet radio/intermittent TCP. AetherEnvelope carries authority semantics.

**Plane 2: Local Event Fabric (MQTT 5.0)** — service coordination on a single machine or LAN. Low-latency pub/sub for high-frequency events (streaming tokens, tool lifecycle, decomposition state). Metadata envelope carries operator/service/instance/schema version.

These planes connect at **styrened** (projects DaemonEvent → MQTT, routes MQTT → LXMF). They are never conflated — MQTT does not replace LXMF, and LXMF does not carry local event fabric traffic.

## Export Configuration

Operator-defined in Auspex config. Each export target specifies:

- **type** — kafka, otlp, syslog, prometheus
- **filter** — MQTT subscription wildcard (e.g. `styrene/+/omegon/+/events/turn.*`)
- **redact** — field paths to strip before export (args, payload, file paths)
- **format** — target-specific serialization (CEF for SIEM, JSON for Kafka, OTLP spans/metrics)
- **cardinality** — bounds on label/attribute dimensions per backend

```toml
[[exports]]
type = "kafka"
bootstrap_servers = "kafka.corp:9092"
topic_prefix = "styrene.events"
filter = "styrene/+/omegon/+/events/#"
redact = ["args", "payload"]

[[exports]]
type = "otlp"
endpoint = "https://otel-collector.corp:4317"
filter = "styrene/+/+/+/events/turn.*"
resource_attributes = { "service.namespace" = "ai-agents" }

[[exports]]
type = "syslog"
host = "siem.corp:514"
filter = "styrene/+/+/+/events/session.*"
format = "cef"
```

## Redaction

Non-negotiable before any event leaves the node. Tool arguments can contain secrets, file paths contain usernames, message chunks contain proprietary code. The export layer applies `omegon-secrets` redaction patterns before serializing to any external format.

## What Exists Today

| Capability | Current state |
|---|---|
| Local event observation | Auspex IPC (msgpack, single client per Omegon) |
| Metrics | Aether Prometheus `/metrics` (mesh + extension health) |
| OTEL | Not implemented (deferred per auspex-telemetry-aggregation.md) |
| SIEM export | Not implemented |
| Kafka export | Not implemented |
| Telemetry aggregation | Auspex normalizes Omegon sensor data into SessionTelemetryData |
| Redaction | omegon-secrets has patterns; not applied to export path |

## Implementation Phases

### Phase 1: Auspex owns the MQTT broker
- Auspex starts an embedded rumqttd broker with TCP listener (:1883) + in-process link
- In-process link feeds the aggregation/export pipeline (no network hop for Auspex's own consumption)
- Omegon instances connect as TCP clients, publish projected AgentEvents (already wired in mqtt_bridge.rs)
- auspex-core gains `styrene-mqtt` with `embedded-broker` feature as a dependency

### Phase 2: Redaction pipeline
- Export-safe event types (stripped of secrets, bounded cardinality)
- Reuses omegon-secrets redaction patterns
- Configurable per-export-target field masks

### Phase 3: OTLP adapter
- Map MQTT events to OTEL spans (session = trace, turn = span, tool = child span)
- Metrics: token usage, turn duration, cache hit rate, tool call frequency
- Resource attributes from ServiceIdentity (operator, service, instance)

### Phase 4: Kafka adapter
- Produce to Kafka topics (one per MQTT event type or configurable mapping)
- JSON serialization with schema registry integration (optional)
- Elasticsearch/OpenSearch index templates for common query patterns

### Phase 5: Syslog/SIEM adapter
- CEF or LEEF format for security events
- Session lifecycle, tool executions, auth state changes, delegation chains
- Severity mapping from QoS/event type

## Dependencies on styrene-mqtt

The MQTT crate stays transport-only. Export adapters live in Auspex. No changes to styrene-mqtt needed for enterprise export — it's already the right abstraction.
