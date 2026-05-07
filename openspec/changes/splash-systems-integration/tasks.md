+++
id = "ede48d02-39ef-4fc3-ac85-75a7dccc5c89"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Splash screen systems check visualization — Tasks

## 1. Startup probe module (`core/crates/omegon/src/startup.rs`) — NEW

- [ ] 1.1 Define `ProbeResult { label: &'static str, state: ProbeState, summary: String }` and `ProbeState { Done, Failed }`
- [ ] 1.2 Define `CapabilityTier` enum: `FullCloud`, `BeefyLocal`, `FreeCloud`, `SmallLocal`, `Offline`
- [ ] 1.3 `probe_cloud()` — check env vars ANTHROPIC_API_KEY, OPENAI_API_KEY, OPENROUTER_API_KEY + auth.json credentials. Summary: "anthropic, openai" or "none"
- [ ] 1.4 `probe_local()` — HTTP GET to localhost:11434/api/tags (Ollama), :1234/v1/models (LM Studio), :8080/v1/models (vLLM/TGI). 100ms connect timeout per port. Summary: "ollama: 7 models" or "not found"
- [ ] 1.5 `probe_hardware()` — macOS: `sysctl -n hw.memsize` for RAM, detect Apple Silicon via `uname -m`. Linux: `nvidia-smi --query-gpu=memory.total --format=csv,noheader,nounits` for VRAM, `/proc/meminfo` for RAM. Summary: "M2 Pro, 32GB" or "16GB RAM, no GPU"
- [ ] 1.6 `probe_memory(cwd)` — count facts in facts.jsonl or memory.db. Summary: "1800 facts" or "empty"
- [ ] 1.7 `probe_tools()` — count registered tool definitions from tool_registry. Summary: "42 tools"
- [ ] 1.8 `probe_design(cwd)` — count .md files in docs/ dir matching design node frontmatter. Summary: "235 nodes"
- [ ] 1.9 `probe_secrets()` — check vault backend reachability, keyring availability. Summary: "vault, 3 stored" or "none"
- [ ] 1.10 `probe_container()` — `podman --version` or `docker --version`. Summary: "podman 5.8.0" or "not found"
- [ ] 1.11 `probe_mcp()` — count configured MCP servers from plugin manifests. Summary: "2 servers" or "none"
- [ ] 1.12 `run_probes(tx, cwd)` — spawn all probes via tokio::join!, send each ProbeResult through tx as it completes. Total must complete within 2s.
- [ ] 1.13 `classify_tier(results) -> CapabilityTier` — derive tier from probe results (cloud keys → FullCloud, local 14B+ model + 32GB RAM → BeefyLocal, etc.)
- [ ] 1.14 Tests: `probe_cloud_with_env_var`, `probe_hardware_doesnt_panic`, `classify_tier_full_cloud`, `classify_tier_offline`, `classify_tier_beefy_local`

## 2. Splash screen expansion (`core/crates/omegon/src/tui/splash.rs`) — MODIFIED

- [ ] 2.1 Add `summary: Option<String>` field to `LoadItem`
- [ ] 2.2 Replace 3 hardcoded items with 9: cloud, local, hardware, memory, tools, design, secrets, container, mcp — all start as `Pending`
- [ ] 2.3 Add `receive_probe(&mut self, result: ProbeResult)` method — maps ProbeResult to the matching LoadItem, sets state and summary
- [ ] 2.4 Replace `render_checklist()` with `render_grid()` — 3 columns × 3 rows. Each cell: indicator + label + parenthetical summary when Done. Dim when Pending, accent when Active, green when Done, red when Failed.
- [ ] 2.5 Grid layout: center horizontally, each column width = max(label+summary) + padding. Handle narrow terminals by falling back to 2 columns or 1 column.
- [ ] 2.6 Update `ready_to_dismiss()` — all 9 items must be Done/Failed
- [ ] 2.7 Tests: `grid_renders_without_panic`, `receive_probe_updates_item`, `nine_items_initialized`

## 3. TUI splash loop integration (`core/crates/omegon/src/tui/mod.rs`) — MODIFIED

- [ ] 3.1 Create `std::sync::mpsc::channel()` for ProbeResult before splash loop
- [ ] 3.2 Spawn `tokio::spawn(startup::run_probes(tx, cwd))` before entering splash loop
- [ ] 3.3 In splash loop: `while let Ok(result) = probe_rx.try_recv() { splash.receive_probe(result); }` each frame
- [ ] 3.4 Remove the cosmetic frame-threshold transitions (frame 8/12/16 Done assignments)
- [ ] 3.5 After splash loop exits: drain remaining probe results from channel, build `CapabilityTier`, store on App for tutorial/routing use
- [ ] 3.6 Add `capability_tier: Option<CapabilityTier>` field to App struct

## 4. Main module wiring (`core/crates/omegon/src/main.rs`) — MODIFIED

- [ ] 4.1 Add `mod startup;`
- [ ] 4.2 Pass CapabilityTier through TuiConfig so it's available to App on construction
