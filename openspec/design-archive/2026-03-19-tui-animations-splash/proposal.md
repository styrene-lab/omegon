+++
id = "0983c34f-8e98-4736-a936-5767658428bb"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# TUI animations and splash screen — tachyonfx + Omegon branding

## Intent

Add smooth animations via tachyonfx and an Omegon splash screen to the TUI. The binary loads in ~50ms so the splash is a branding moment, not a loading screen — user should be able to disable via --no-splash or settings. Animations should enhance the sci-fi aesthetic without adding latency to the interaction loop.

See [design doc](../../../docs/tui-animations-splash.md).
