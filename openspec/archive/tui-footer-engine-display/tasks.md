+++
id = "fe668a56-8e8a-4f3b-a14f-a29f89faf93e"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Footer redesign — engine display + linked minds — Tasks

## 1. instruments.rs — rendering core (new file, no deps)

- [ ] 1.1 Create `tui/instruments.rs` with `InstrumentPanel` struct holding four instrument renderers + telemetry state
- [ ] 1.2 Port Perlin sonar renderer from fractal_demo with operator-tuned params (scale=7.9, octaves=2.5, lac=4.0, amp=1.0)
- [ ] 1.3 Port Lissajous radar renderer with operator-tuned params (curves=3.6, freq=1.9, spread=3.0, amp=0.5, pts=500)
- [ ] 1.4 Port Plasma thermal renderer with operator-tuned params (complexity=2.46, distortion=0.68, amp=1.0). Quadratic speed ramp for ignition effect.
- [ ] 1.5 Port CA waterfall renderer with CRT noise glyphs, per-mind WaterfallState columns, Rule 204/30/110/90/150 per op
- [ ] 1.6 Implement CIE L* perceptual `intensity_color()` ramp: navy(0.0)→teal(0.3-0.5)→amber(0.5-1.0). Amber gets 50% of perceptual range.
- [ ] 1.7 Implement `InstrumentPanel::render()` — 2×2 grid layout with labeled borders, intensity% in title
- [ ] 1.8 Implement `InstrumentPanel::update_telemetry()` — takes context_pct, tool events, thinking level, memory mind states
- [ ] 1.9 Tool error state: amber body + red border on radar instrument
- [ ] 1.10 Context instrument caps at 70% intensity (auto-compaction threshold)
- [ ] 1.11 Tests: all four instruments render without panic at 22×5, color ramp produces expected values, per-mind waterfall independence

## 2. footer.rs — split-panel layout (depends on 1)

- [ ] 2.1 Rewrite footer as split-panel: left 40% (engine + memory), right 60% (system state with instruments)
- [ ] 2.2 Engine panel: model name, provider, auth, tier, thinking level, context mode, context gauge. All values always visible (show zeros).
- [ ] 2.3 Memory/Minds panel: table of linked minds — name, fact count, injection state, token weight per mind
- [ ] 2.4 System state panel: embed InstrumentPanel 2×2 grid + stats column (cwd, git branch, turns, tools, compactions, MCP, uptime)
- [ ] 2.5 Git branch display: current branch name, clean/dirty indicator. Uses `git rev-parse --abbrev-ref HEAD` and `git status --porcelain`
- [ ] 2.6 Compact fallback: at terminal height < 35 rows, revert to simplified 3-row status bar
- [ ] 2.7 Tests: footer renders at 120×12, 160×12, 80×5 (compact), all data fields populated

## 3. mod.rs — layout + wiring (depends on 1 and 2)

- [ ] 3.1 Change vertical layout: footer from Length(5) to Length(12). Remove hint line constraint (absorbed into engine panel).
- [ ] 3.2 Add `/focus` slash command and hotkey toggle — hides footer entirely, conversation gets full height
- [ ] 3.3 Wire telemetry: context_percent, tool call events, thinking level, memory operations → InstrumentPanel::update_telemetry()
- [ ] 3.4 Wire per-mind memory events: memory_store/recall/focus/release/episodes/archive → specific mind columns
- [ ] 3.5 Add to bg cleanup pass allow-list: any new colors from instruments
- [ ] 3.6 Update the `draw()` method to pass InstrumentPanel to footer render

## 4. dashboard.rs + fractal.rs — cleanup (depends on 1)

- [ ] 4.1 Remove fractal rendering from dashboard header. Dashboard sidebar gains ~8 rows.
- [ ] 4.2 Gut fractal.rs — remove FractalWidget, AgentMode, old renderers. Keep file as stub or delete.
- [ ] 4.3 Remove fractal_area from DashboardState and from bg cleanup pass exclusion zone
- [ ] 4.4 Update `tui/mod.rs` pub mod declarations

## 5. theme + config (no deps)

- [ ] 5.1 Update alpharius.json with any new instrument panel color variables
- [ ] 5.2 Ensure theme.rs hardcoded fallback matches alpharius.json values
- [ ] 5.3 Verify footer_bg is used consistently (not card_bg) in all footer rendering

## 6. docs + README (depends on all above)

- [ ] 6.1 Update README.md with CIC instrument panel description and feature list
- [ ] 6.2 Update CHANGELOG.md via git-cliff or manual entry
- [ ] 6.3 Update design tree node status to implemented

## Cross-cutting constraints

- [ ] C.1 Must work at minimum terminal width 120 cols
- [ ] C.2 Focus mode toggle must be instant — no re-layout delay
- [ ] C.3 Compact fallback at terminal heights under 35 rows
- [ ] C.4 All four instruments must render in under 1ms combined at 60fps
- [ ] C.5 Per-mind waterfall columns must update independently
- [ ] C.6 Tool error state must show red border on radar instrument
- [ ] C.7 alpharius.json is the authoritative theme source for ALL color values
