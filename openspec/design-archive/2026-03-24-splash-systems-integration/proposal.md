+++
id = "0674b88f-f01e-4b34-b567-4a2f1e17ba60"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Splash screen systems check visualization — real loading behind the animation

## Intent

Replace the cosmetic splash loading checklist with real systems check progress. The splash animation runs ~1.7s; the systems check (GPU detection, Ollama probe, port scanning, provider auth check, memory loading) takes 100-500ms. Run them in parallel — the animation masks the latency, and each checklist item transitions from scanning → done/failed as the actual probe completes. The user sees Omegon genuinely discovering its environment, not a fake loading bar.

See [design doc](../../../docs/splash-systems-integration.md).
