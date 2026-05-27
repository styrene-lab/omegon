# Tasks: Extension Operator Surface Contributions

## 1. SDK/protocol schema
<!-- specs: extensions/operator-surface-contributions -->

- [ ] 1.1 Add `ui_contributions` capability defaulting false.
- [ ] 1.2 Add SDK structs for `UiContributionSet`, namespace, commands, status items, and surfaces.
- [ ] 1.3 Add delegated Reader JSON round-trip test.
- [ ] 1.4 Add host-rendered Scratchpad list JSON round-trip test.

## 2. Manifest envelope parsing
<!-- specs: extensions/operator-surface-contributions -->

- [ ] 2.1 Parse `[ui]` namespace/description.
- [ ] 2.2 Parse `[[ui.commands]]` entries.
- [ ] 2.3 Parse `[[ui.status_items]]` entries.
- [ ] 2.4 Parse `[[ui.surfaces]]` entries with rendering and placements.
- [ ] 2.5 Add TOML tests for Reader manifest envelope.
- [ ] 2.6 Add TOML tests for Scratchpad host-rendered list envelope.

## 3. Runtime discovery and validation
<!-- specs: extensions/operator-surface-contributions -->

- [ ] 3.1 Call `ui/list_contributions` only when capability is enabled.
- [ ] 3.2 Validate runtime contributions against manifest envelope.
- [ ] 3.3 Validate tool ownership.
- [ ] 3.4 Reject raw drawing/unsupported contribution kinds.
- [ ] 3.5 Store accepted contributions in a host registry.
- [ ] 3.6 Expose accepted/rejected diagnostics.

## 4. Reader command/status MVP
<!-- specs: extensions/operator-surface-contributions -->

- [ ] 4.1 Register accepted Reader command contributions into namespaced slash routing.
- [ ] 4.2 Route Reader status command to `reader_status`.
- [ ] 4.3 Route Reader open command to `reader_open` with placement args.
- [ ] 4.4 Preserve deterministic namespace conflict fallback.

## 5. Host-rendered primitive list MVP
<!-- specs: extensions/operator-surface-contributions -->

- [ ] 5.1 Define minimal `list` primitive schema.
- [ ] 5.2 Render host-owned list from extension data tool response.
- [ ] 5.3 Support title/subtitle/badge field interpolation.
- [ ] 5.4 Support one host-owned action routed to an extension tool.

## 6. Validation
<!-- specs: extensions/operator-surface-contributions -->

- [ ] 6.1 Run `cargo test -p omegon-extension ui_contribution -- --nocapture`.
- [ ] 6.2 Run `cargo test -p omegon operator_surface -- --nocapture`.
- [ ] 6.3 Run `just lint`.
