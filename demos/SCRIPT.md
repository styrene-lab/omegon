+++
id = "b5372dc2-4ca6-4619-a81a-eceb4b59eb9a"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Omegon — asciinema demo script

Record a ~90-second demo showing Omegon reading code, finding a bug,
and fixing it — all inside the TUI.

## Prerequisites

```bash
# Install recording tools
cargo install asciinema        # or: brew install asciinema / dnf install asciinema
cargo install agg              # asciinema gif generator (optional, for GIF output)
```

## Setup

```bash
# From the demos/ directory:
./setup.sh          # creates the sample project in /tmp/omegon-demo-rec
```

## Recording

```bash
./record.sh         # starts asciinema recording inside the demo project
```

This drops you into a shell inside `/tmp/omegon-demo-rec`.
Follow the beats below, then `exit` to stop recording.

## Beats

### 1. Launch (5s)

```
om
```

Wait for the TUI to render. The splash screen appears briefly, then the
editor is ready.

### 2. Ask Omegon to read the project (15s)

Type into the editor:

```
Read this project and tell me if there are any bugs.
```

Watch the tool cards appear — Omegon reads `Cargo.toml`, `src/main.rs`,
and `src/lib.rs`. It spots the off-by-one in the `fibonacci()` loop
(`2..n` should be `1..=n`) and notes that `test_sequence` fails.

### 3. Ask Omegon to fix the bug (20s)

```
Fix the fibonacci bug.
```

Omegon edits `src/lib.rs`, correcting the loop bound. A tool card shows
the diff. It then runs `cargo test` to verify — all tests pass.

### 4. Ask Omegon to commit (10s)

```
Commit the fix.
```

Omegon stages the change and creates a commit with a clear message.

### 5. Exit (5s)

Press `Ctrl+C` twice or type `/quit`.

## Post-processing

```bash
# The .cast file is at demos/omegon-demo.cast

# Convert to GIF (if agg is installed):
agg omegon-demo.cast omegon-demo.gif --cols 160 --rows 50

# Or upload to asciinema.org:
asciinema upload omegon-demo.cast
```

## Tips

- Use a clean terminal with a dark background (the TUI themes assume dark)
- Set terminal to 160×50 or larger
- Pause a beat after each prompt so the viewer can read it before output streams
- If you flub a take, `Ctrl+C` out and re-run `./record.sh` (it overwrites)
