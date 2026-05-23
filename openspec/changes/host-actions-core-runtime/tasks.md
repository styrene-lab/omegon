# HostActions Core Runtime Tasks

## 1. Manifest and capability substrate
<!-- specs: extensions/host-actions-runtime -->

- [x] 1.1 Add failing manifest tests for HostAction capability flags defaulting false and parsing true.
- [x] 1.2 Add failing manifest tests for `[permissions.host_actions] allowed` parsing.
- [x] 1.3 Add failing manifest tests for `[permissions.host_actions.terminal_create]` constraints.
- [x] 1.4 Implement manifest structs and serde defaults without breaking existing manifests.
- [x] 1.5 Expose loaded manifest HostAction permissions to extension runtime context.

## 2. Structured tool-result envelope extraction
<!-- specs: extensions/host-actions-runtime -->

- [x] 2.1 Add failing tests for legacy raw JSON/string result compatibility.
- [x] 2.2 Add failing tests for `{content:[...]}` extraction to ordinary `ContentBlock`s.
- [x] 2.3 Add failing tests for valid `actions` extraction separate from content.
- [x] 2.4 Add failing tests proving malformed actions preserve content and produce invalid/ignored outcomes.
- [x] 2.5 Implement envelope parser and wire it into `ExtensionFeature::execute`.
- [x] 2.6 Harden envelope parsing so malformed content falls back to visible legacy text.
- [x] 2.7 Accept both `media_type` and `mediaType` image content fields.

## 3. Validation and policy pipeline
<!-- specs: extensions/host-actions-runtime -->

- [x] 3.1 Add failing tests for invalid action candidates.
- [x] 3.2 Add failing tests for unsupported action type/version outcomes.
- [x] 3.3 Add failing tests for manifest-denied action outcomes.
- [x] 3.4 Add failing tests for conservative `auto_if_allowed` behavior.
- [x] 3.5 Implement origin/scoped identity model, policy checks, typed outcomes, and audit hook.

## 4. Imperative `actions/execute` route
<!-- specs: extensions/host-actions-runtime -->

- [ ] 4.1 Add failing tests proving `actions/execute` uses the same pipeline as declarative actions.
- [ ] 4.2 Add failing tests for denied/invalid/unsupported imperative outcomes.
- [ ] 4.3 Implement extension host request routing for `actions/execute` without a bypass path.

## 5. Rendering/headless result exposure
<!-- specs: extensions/host-actions-runtime -->

- [ ] 5.1 Add failing tests for host action details/outcomes separate from ordinary content.
- [ ] 5.2 Add failing tests for headless deterministic result details.
- [ ] 5.3 Implement minimal rendering/detail schema for declarative actions and outcomes.
- [ ] 5.4 Defer rich TUI/ACP action cards unless required by acceptance review.

## 6. Validation and upstream closure
<!-- specs: extensions/host-actions-runtime -->

- [ ] 6.1 Run `cargo test -p omegon-extension`.
- [ ] 6.2 Run `cargo test -p omegon`.
- [ ] 6.3 Run `cargo check -p omegon`.
- [ ] 6.4 Run `just link` if installing locally.
- [ ] 6.5 Post acceptance trace to issue #75 and close only after all criteria map to code/tests.
