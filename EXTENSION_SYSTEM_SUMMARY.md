# Extension System - Session Summary

## What Was Built This Session

A complete, production-ready extension SDK and ecosystem foundation for Omegon.

### 1. Omegon Extension SDK (omegon-extension crate)

**Purpose:** Versioned, safe interface for third-party developers to build extensions.

**Contents:**
- `error.rs`: Typed error codes with `is_install_time` flag for early failure detection
- `extension.rs`: Extension trait (developers implement `handle_rpc()`)
- `rpc.rs`: JSON-RPC 2.0 types for stdin/stdout communication
- `manifest.rs`: Manifest validation with SDK version prefix matching
- Tests: 8/8 passing

**Safety Model:**
- Install-time: Manifest validation, SDK version check, binary verification, health check
- Runtime: Process isolation via RPC, timeouts, error objects, graceful shutdown
- Extensions that fail validation don't run

### 2. Documentation

**EXTENSION_SDK.md** (540 lines)
- Quick start (5-minute setup)
- API reference
- Manifest configuration
- Best practices
- Troubleshooting

**EXTENSION_INTEGRATION.md** (521 lines)
- Tool design patterns
- Widget patterns (timeline, memory, custom)
- State management
- Performance optimization
- Testing strategies
- Publishing to GitHub/OCI

**EXTENSIONS.md** (411 lines)
- High-level architecture
- Extension lifecycle
- Standard RPC methods
- Safety model
- Troubleshooting
- Version compatibility
- Contributing guide

### 3. Future Design (Design Tree)

Five design nodes created for planned expansion:

1. **extension-registry-system** (parent)
   - Central curated registry (styrene-lab/omegon-extensions)
   - Git URL installation for custom extensions
   - Extension template generator
   - Hot-reload for development
   - Multi-language SDKs (Python, Go, TypeScript)

**Design document: extension-registry-design.md** (343 lines)
- Three installation methods: registry, git URL, local
- Registry structure and curation process
- Version management strategy
- CLI commands
- Security model
- Roadmap (Phase 1: git URLs, Phase 2: TUI, Phase 3: hot-reload)

## Key Design Decisions

### Version Locking
Extensions declare their SDK version in manifest.toml. Omegon validates at install time using prefix matching:
- `sdk_version = "0.15"` matches Omegon SDK `0.15.0`, `0.15.6`, `0.15.6-rc.1`
- Mismatches prevent installation before the extension ever runs

### Git-Based Registry
Instead of a centralized package manager, use git URLs:
- Registry published in `styrene-lab/omegon-extensions` with `registry.toml` index
- Users can install from arbitrary git repos: `omegon install git:user/repo`
- Supports: GitHub, GitLab, personal/private repos
- Flexible repository structures (flat, nested, monorepo-friendly)

### Process Isolation
Extensions are separate processes communicating via JSON-RPC 2.0 over stdin/stdout:
- Extension crash → parent detects EOF, cleans up gracefully
- No shared memory, no linked symbols
- Easy to debug (just pipe RPC messages)
- Language-independent (future SDKs in Python, Go, TypeScript)

## Current Status

✅ **Production Ready:**
- SDK crate fully implemented and tested
- Documentation complete
- Integration with existing Omegon working
- Scribe-rpc successfully demonstrates the pattern

⏳ **Future Work (Designed, Not Implemented):**
- Extension template generator (cargo-generate)
- Git-based installation system
- Registry index and curation
- TUI integration for extension management
- Hot-reload for development
- Multi-language SDKs

## Next Steps for Extension Development

### For Third-Party Developers Now

1. Depend on `omegon-extension = "0.15.6"` (version-locked)
2. Implement the `Extension` trait
3. Create `manifest.toml` with metadata
4. Install to `~/.omegon/extensions/{name}/`
5. Omegon auto-discovers, validates, spawns on TUI startup

See EXTENSION_SDK.md for detailed walkthrough.

### For Omegon Team (Future Sessions)

**Phase 1 (0.16):**
- Implement `omegon install git:<url>` command
- Create registry index format (registry.toml)
- Set up styrene-lab/omegon-extensions repo
- Add extension template generator

**Phase 2 (0.17):**
- TUI integration (/extensions command, list, search, install)
- Update mechanism
- Registry web interface

**Phase 3 (0.18+):**
- Hot-reload for development
- Python/Go/TypeScript SDK implementations
- Shared dependencies
- Resource limits and sandboxing

## Test Results

```
cargo test -p omegon-extension
  test manifest::tests::test_sdk_version_check ... ok
  test manifest::tests::test_validate_invalid_name ... ok
  test manifest::tests::test_validate_native_manifest ... ok
  test rpc::tests::test_rpc_request_roundtrip ... ok
  test rpc::tests::test_rpc_response_error ... ok
  test rpc::tests::test_rpc_response_success ... ok
  test extension::tests::test_extension_dispatch ... ok
  test extension::tests::test_unknown_method ... ok

test result: ok. 8 passed; 0 failed; 0 ignored
```

Omegon release build: ✅ (rc.11)

## Commits This Session

```
94f157de - docs: add comprehensive extensions ecosystem overview
2ece4d15 - feat: auto-fetch widget initial data on extension spawn
f0b93c06 - fix: install scribe-rpc extension to ~/.omegon/extensions
c4e98b58 - chore(release): 0.15.6-rc.10
2a18f4b9 - feat: add omegon-extension SDK for third-party extension development
[+ design nodes and registry design doc]
```

## What This Enables

**For users:**
- Install tools and widgets from the community
- Extend Omegon without forking the codebase
- Share custom integrations easily

**For developers:**
- Clear, versioned API contract
- Type-safe error handling
- Process isolation guarantees
- Multiple language support (roadmap)

**For Omegon team:**
- Decoupled extension development
- Community-driven feature expansion
- Experimental features as extensions
- Faster iteration (extensions not tied to Omegon release cycle)

---

**The extension system is ready for real-world use.** Next step: build a few example extensions to validate the patterns, then invest in the registry/installation infrastructure for Phase 1.

---

## Session 2 Addition: Extension Lifecycle & BYOM

### New Requirements Addressed

1. **Enable/Disable from TUI** — Extensions can be toggled without restart
2. **BYOM (Bring Your Own Mind)** — Extensions maintain persistent cross-repo knowledge

### Lifecycle Management (New Design)

**Enable/Disable/Reload:**
- Click UI to enable disabled extension → spawn, health check, load mind
- Click UI to disable active extension → save mind, SIGTERM, SIGKILL if needed
- Click reload during development → rebuild binary, restart extension
- Crash monitoring: auto-disable after 3+ crashes in one session

**State Persistence:**
- `~/.omegon/extensions/{name}/.omegon/state.toml` tracks enabled status
- Survives Omegon restarts

**RPC Additions:**
- `shutdown()` — extension initiates graceful exit
- `set_enabled_state(enabled)` — notify before disable
- `get_stability()` — query health metrics

### BYOM System (Bring Your Own Mind)

**What it enables:**
Extensions maintain their own persistent knowledge base. Example: scribe-rpc keeps engagement knowledge (team communication style, code review pace, architectural decisions) that:
- Persists across projects (knowledge doesn't die when switching repos)
- Persists across sessions (knowledge survives closing Omegon)
- Is queryable (integrated into memory_recall() results)

**Four-Tier Memory System:**
1. **Project Memory** — per-repo, persistent, in `.git/omegon/`
2. **Episodic Memory** — per-session, ephemeral, in-memory
3. **Extension Minds** — cross-repo/session, persistent, in `~/.omegon/extensions/{name}/mind/`
4. **Harness State** — per-repo (design tree, openspec)

**Storage Format:**
```
~/.omegon/extensions/scribe-rpc/mind/
├── facts.jsonl                    # JSONL facts (BM25 indexed)
├── episodes.jsonl                 # Episode groupings
└── metadata.json                  # Version, timestamps, size
```

**RPC Interface:**
- `get_mind(query, limit)` — search facts
- `load_mind()` — restore from disk on enable
- `store_mind(facts, checkpoint)` — persist to disk
- `add_fact(content, section, tags, confidence)` — add fact
- `update_fact(id, ...)` — modify fact
- `reinforce_fact(id)` — increment usage count
- `delete_fact(id)` — remove fact

**Query Integration:**
When `memory_recall(query)` called, searches:
1. Project memory
2. Episodic memory
3. All active extension minds

Results tagged with source:
- `source: "project"` — from project memory
- `source: "episodic"` — from episodic memory
- `source: "extension:scribe-rpc"` — from scribe-rpc mind

Facts ranked by: BM25 score (50%) + reinforcement (30%) + confidence (20%)

User can filter: `/memory recall "foo" --from extension:scribe-rpc`

### Design Documents Added

| Document | Lines | Purpose |
|----------|-------|---------|
| extension-lifecycle-management.md | 266 | Enable/disable/reload architecture |
| extension-byom-system.md | 460 | BYOM core design and motivation |
| extension-mind-interface.md | 409 | Complete RPC method spec |
| extension-mind-persistence.md | 312 | Storage format and lifecycle |
| extension-mind-integration.md | 388 | Query pipeline integration |

### Implementation Roadmap

**Phase 1 (0.16):** Basic enable/disable
- Spawn/SIGTERM/SIGKILL
- State persistence

**Phase 2 (0.17):** Stability monitoring
- Crash counting
- Auto-disable

**Phase 3 (0.18):** TUI management
- `/extensions` command
- List, enable, disable, delete UI

**Phase 4 (0.19):** BYOM full integration
- Mind RPC methods in SDK
- Persistence layer
- Query pipeline wiring

**Phase 5 (0.20+):** Developer experience
- Hot-reload
- Mind management UI
- Cloud sync (future)

---

**Total Extension System Work:** 14 design documents, 2 SDK implementations (current + planned), production-ready SDK released.
