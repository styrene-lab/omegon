+++
id = "2545fe5c-8872-4ad3-ac23-225914843203"
kind = "document"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
design_docs = ["design/operator-capability-profile.md", "design/guardrail-capability-probe.md", "design/bootstrap.md"]
last_updated = "2026-03-10"
openspec_baselines = ["models/profile.md"]
subsystem = "operator-profile"
+++

# Operator Profile

> Provider login, local hardware assessment, capability discovery, and routing preferences — the operator's contract with the inference layer.

## What It Does

The operator profile system captures durable preferences about which providers and models the operator wants Omegon to use. It separates two concerns:

1. **Capability discovery**: What providers are authenticated? What local models are available? What hardware (GPU, RAM) exists?
2. **Routing preferences**: Which providers are preferred for each capability role? When should fallback be blocked vs. allowed?

Bootstrap runs on first session start, probing dependencies (d2, pandoc, pdftoppm, Ollama) and provider readiness (API keys, OAuth tokens). The capability profile gates fallback decisions — if the operator hasn't approved a provider, transient failures on the primary won't silently fall over to it.

## Key Files

| File | Role |
|------|------|
| `extensions/bootstrap/index.ts` | First-run setup, dependency probing, onboarding flow |
| `extensions/bootstrap/deps.ts` | Dependency checks — d2, pandoc, pdftoppm, Ollama, clipboard tools |
| `extensions/lib/operator-profile.ts` | Profile schema, role-to-candidate mapping, fallback policy |
| `extensions/lib/operator-fallback.ts` | Alternate candidate resolution using profile preferences |
| `extensions/lib/model-preferences.ts` | Persisted model preferences (`.omegon/profile.json`) |

## Design Decisions

- **Profile gates fallbacks, not just advertises capabilities**: A provider must be in the operator's preference list to receive fallback traffic. Silent cross-provider fallback is blocked by default.
- **Schema maps semantic roles to ordered concrete candidates**: Each role (driver, extraction, compaction, review) has an ordered list of acceptable model candidates.
- **Provider/source and role/tier are separate axes**: "Use Anthropic for driver" and "use opus tier" are independent choices.
- **Default profile is safe without setup**: Without configuration, Omegon uses the authenticated provider's defaults — no fallback, no local inference unless explicitly enabled.
- **Fixed 5-minute cooldown for transient provider failures in v1**: Simple timer-based cooldown before retrying a failed provider.

## Behavioral Contracts

See `openspec/baseline/models/profile.md` for Given/When/Then scenarios covering role resolution and fallback policy.

## Constraints & Known Limitations

- Profile is per-repo (`.omegon/profile.json`), not per-machine
- No automatic provider re-authentication — if an API key expires, operator must reconfigure
- Hardware detection is macOS-focused (sysctl for GPU, system_profiler for RAM)

## Related Subsystems

- [Model Routing](model-routing.md) — consumes profile for provider resolution
- [Error Recovery](error-recovery.md) — uses fallback policy for recovery decisions
- [Dashboard](dashboard.md) — displays provider status
