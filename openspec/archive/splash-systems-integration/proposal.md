+++
id = "753ce1e3-8fa6-48ea-a76e-d1119952daff"
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
