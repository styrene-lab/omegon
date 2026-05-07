+++
id = "12b7e023-ad30-4468-a1a9-a99aba5dbb30"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# TUI surface pass — Tasks

## 1. Dashboard harness section (dashboard.rs)
- [x] 1.1 Harness status section: persona/tone, providers, MCP, secrets, inference, container
- [x] 1.2 Cleave section conditional — hides entirely when idle
- [x] 1.3 Vertical stack: design tree → openspec → cleave (if active) → harness

## 2. Context class selector (selector.rs)
- [x] 2.1 SelectorKind::ContextClass with 4 options + token counts + descriptions
- [x] 2.2 On confirm: update settings.context_class + context_window

## 3. Toast notifications + event handling (mod.rs)
- [x] 3.1 /context opens context class selector overlay
- [x] 3.2 Toast on persona switch, tone change, MCP connect/disconnect
- [x] 3.3 Dashboard refresh on HarnessStatusChanged
- [x] 3.4 Previous HarnessStatus snapshot diffing (PartialEq on PersonaSummary/ToneSummary)

## 4. Compaction flash (footer.rs)
- [x] 4.1 compaction_flash_ticks counter, accent border pulse for 3 ticks

## 5. Cross-cutting
- [x] 5.1 Cleave hides entirely when idle
- [x] 5.2 Harness section reads from FooterData.harness
- [x] 5.3 Context class selector shows token count + description
- [x] 5.4 Toasts compare previous snapshot, don't fire on every event
