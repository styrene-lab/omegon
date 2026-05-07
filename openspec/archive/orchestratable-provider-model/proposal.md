+++
id = "2368a080-25a9-45a5-b2f7-d3eeb8fdf0c2"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Orchestratable provider model — treat providers as assignable resources, not user preferences

## Intent

Transform provider handling from 'pick one at startup, fallback if it fails' to 'maintain an inventory of available providers, assign them to tasks based on cost/capability/latency requirements during orchestration'. The single Arc<RwLock<Box<dyn LlmBridge>>> becomes a ProviderPool. Cleave children get per-task provider assignments. Local inference becomes a schedulable resource with VRAM awareness. The harness becomes a router, not a client.
