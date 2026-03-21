# Version switcher — Tasks

## 1. core/crates/omegon/src/switch.rs (new)

- [ ] 1.1 GitHub Releases client — fetch release list from styrene-lab/omegon-core, parse tag names into semver, distinguish stable vs RC (`-rc.N`), cache response for session
- [ ] 1.2 Platform detection — detect current OS/arch, map to release artifact name (`omegon-{target}.tar.gz`)
- [ ] 1.3 Download + verify — download tarball from release assets, fetch `checksums.sha256`, verify SHA256 match, extract binary to `~/.omegon/versions/{version}/omegon`
- [ ] 1.4 Version storage — `~/.omegon/versions/` directory management: list installed versions, identify active version (resolve current_exe symlink), cleanup/remove old versions
- [ ] 1.5 Symlink activation — replace the binary at `current_exe()` (or its symlink target) with a symlink to the versioned binary. Handle the case where current_exe is not a symlink (first-time setup: move current binary into versions dir, replace with symlink)
- [ ] 1.6 Interactive picker — terminal UI (no ratatui, raw crossterm) showing available versions with installed/active markers. Arrow keys to navigate, enter to select. Group: stable releases, then RCs. Show `(installed)` and `● active` markers
- [ ] 1.7 `.omegon-version` auto-detect — walk cwd ancestors looking for `.omegon-version` file, parse version string, compare against active version, warn if mismatch. Called at startup (not just from switch subcommand)
- [ ] 1.8 Tests — unit tests for version parsing, platform detection, symlink resolution, .omegon-version parsing

## 2. core/crates/omegon/src/main.rs (modified)

- [ ] 2.1 Add `Switch` variant to `Commands` enum with args: optional version string, `--list` flag, `--latest` flag, `--latest-rc` flag
- [ ] 2.2 Wire switch subcommand handler: dispatch to switch module functions based on args
- [ ] 2.3 Add `.omegon-version` check at interactive startup — call auto-detect, show warning if version mismatch

## 3. core/install.sh (modified)

- [ ] 3.1 Change install target from flat `/usr/local/bin/omegon` to `~/.omegon/versions/{version}/omegon` with symlink at install location
- [ ] 3.2 Ensure backward compat — if upgrading from flat binary, move it into versions dir first
