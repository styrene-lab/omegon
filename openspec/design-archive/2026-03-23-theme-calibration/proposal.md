+++
id = "6241a4fa-ccb6-492b-b7fd-646cada2d8c1"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Theme calibration — /calibrate command, gamma/sat/hue slider, tweakcn-style theme export

## Intent

Alpharius is a strong opinionated theme but doesn't account for display variation (dim laptop screens, ultra-wide monitors, terminal emulator differences). Add a /calibrate slash command that lets operators adjust gamma, saturation, and hue shift — persisted to settings. Look at shadcn's tweakcn (https://tweakcn.com) for the import/export model: lists of CSS color values that can be shared. Create an alpharius/omegon theme set on tweakcn as a distribution channel. The Styrene Python TUI already did this pattern. The calibration UI could be a TUI overlay with live preview — operator sees changes immediately as they adjust sliders.

See [design doc](../../../docs/theme-calibration.md).
