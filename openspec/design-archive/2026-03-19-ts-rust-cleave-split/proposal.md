# TS/Rust cleave split — experimental Rust backend, open-sourced TS harness, clean migration boundary

## Intent

The TS→Rust migration is half-done. Cleave currently dispatches TS children (node bin/omegon.mjs) that hit the same provider bugs the Rust binary has already fixed. Rather than keep patching both codebases in lockstep, create an explicit fork in the road: the Rust binary becomes an opt-in experimental cleave backend (where it should be as a maturing implementation), the TS project gets open-sourced for the pi community who already knows it, and the pure-Rust path is there as an easter egg for anyone who discovers experimental cleave. This cleanly separates the two lifecycles instead of keeping them entangled.

See [design doc](../../../docs/ts-rust-cleave-split.md).
