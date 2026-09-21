# Implementation tasks

## 1. Quiet first launch
<!-- specs: fresh-startup -->

- [x] Reproduce the legacy menu with a profile-free, non-child captured launch.
- [x] Remove the blocking wizard and automatic posture write; retain splash detection.
- [x] Verify both entrypoint defaults with zero-inference startup captures.
- [x] Run the omegon crate gate, changed Clippy, script tests, and OpenSpec validation.
- [x] Remove the mistakenly created local posture override and refresh the existing preview.

- [x] Isolate the permission AlwaysAllow test in a temporary project profile; verify the user-profile hash remains unchanged.
