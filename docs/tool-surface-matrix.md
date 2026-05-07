+++
id = "dde770f0-0459-4902-a680-e0cc49561c4c"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Tool Surface Matrix — External Binary Dependencies

Every external binary that omegon shells out to at runtime, organized by crate and module.
Native Rust implementations (libgit2, reqwest, tree-sitter, etc.) are not listed.

Last updated: 2026-04-16

## Legend

| Status | Meaning |
|--------|---------|
| **REQUIRED** | Hard failure if missing |
| **GUARDED** | Checked before use; graceful error if missing |
| **OPTIONAL** | Fallback path exists; works without it |
| **TEST-ONLY** | Only used in `#[cfg(test)]` code |
| **JJ-ONLY** | Only called inside `is_jj_repo()` guard |

---

## omegon (main crate)

### tools/bash.rs + tools/native_cmd.rs
| Binary | Call | When | Status |
|--------|------|------|--------|
| `bash` | `bash -c <command>` | Commands with shell syntax or unknown commands | **REQUIRED** |
| *(native)* | In-process dispatch | Simple commands (see table below) | Native |

**Native dispatch** intercepts common single-command invocations before spawning bash.
Commands containing pipes, redirects, variable expansion, or shell metacharacters
always fall through to bash. Unknown commands and unrecognized flags also fall through.
The `details` field includes `"native": true` when dispatch handles the command.

**Natively dispatched commands:**

| Command | Flags supported | Implementation |
|---------|----------------|----------------|
| `cat` | (no flags) | `std::fs::read_to_string` |
| `head` | `-n N`, `-N` | `BufRead::lines().take(n)` |
| `tail` | `-n N`, `-N` | Read + take last N |
| `wc` | `-l`, `-w`, `-c` | `std::fs` byte/line/word count |
| `ls` | `-a`, `-l`, `-1` | `std::fs::read_dir` sorted |
| `find` | `-name`, `-type f\|d` | `ignore` crate walker (no gitignore) |
| `grep` | `-r`, `-n`, `-i`, `-l`, `-c`, `-v` | `grep-regex` + manual line matching |
| `mkdir` | `-p` | `std::fs::create_dir_all` |
| `touch` | (no flags) | `File::set_modified(now)` |
| `rm` | `-r`, `-f` | `std::fs::remove_file/dir_all` (with safety checks) |
| `cp` | `-r` | `std::fs::copy` + recursive with symlink preservation |
| `mv` | (no flags) | `std::fs::rename` with cross-device fallback |
| `sort` | `-r`, `-u`, `-n` | `Vec::sort` with options |
| `basename` | (no flags) | `Path::file_name()` |
| `dirname` | (no flags) | `Path::parent()` |
| `realpath` | (no flags) | `std::fs::canonicalize` |
| `echo` | (no flags) | Direct string output |
| `pwd` | — | Returns cwd |
| `true` | — | Exit 0 |
| `false` | — | Exit 1 |

### tools/speculate.rs
| Binary | Call | When | Status |
|--------|------|------|--------|
| *(none at runtime)* | All operations use libgit2 natively | — | — |
| `git` | Various (init, config, add, commit) | Test setup only | **TEST-ONLY** |

### tools/codebase_search.rs
| Binary | Call | When | Status |
|--------|------|------|--------|
| *(none)* | HEAD check uses libgit2 `Repository::discover()` | Background reindex | Native |

### tools/view.rs
| Binary | Call | When | Status |
|--------|------|------|--------|
| `pdftotext` | `pdftotext -layout <file> -` | Viewing PDF files | **GUARDED** |
| `pandoc` | `pandoc -f <format> -t markdown` | Viewing DOCX/XLSX/PPTX/EPUB/ODT/RTF | **GUARDED** |

### tools/render.rs
| Binary | Call | When | Status |
|--------|------|------|--------|
| `d2` | `d2 --layout elk --theme 200` | Rendering D2 diagrams | **GUARDED** |
| `python3` | `python3 -m mlx_flux` | FLUX.1 image generation (Apple Silicon) | **GUARDED** |

### tools/whoami.rs
| Binary | Call | When | Status |
|--------|------|------|--------|
| `gh` | `gh auth status` | GitHub auth check | **GUARDED** |
| `glab` | `glab auth status` | GitLab auth check | **GUARDED** |
| `aws` | `aws sts get-caller-identity` | AWS auth check | **GUARDED** |
| `kubectl` | `kubectl config current-context`, `cluster-info` | K8s auth check | **GUARDED** |
| `podman` | `podman login --get-login` | Container registry check | **GUARDED** |
| `docker` | `docker login --get-login` | Container registry fallback | **GUARDED** |
| `vault` | `vault token lookup` | Vault auth check | **GUARDED** |

### tools/local_inference.rs
| Binary | Call | When | Status |
|--------|------|------|--------|
| `open` | `open -a Ollama` | Starting Ollama (macOS) | **OPTIONAL** |
| `ollama` | `ollama serve` | Starting Ollama (fallback) | **GUARDED** |
| `osascript` | `tell application "Ollama" to quit` | Stopping Ollama (macOS) | **OPTIONAL** |
| `pkill` | `pkill -x ollama` | Stopping Ollama (fallback) | **OPTIONAL** |

### tools/mod.rs (commit handler)
| Binary | Call | When | Status |
|--------|------|------|--------|
| *(none)* | Branch query uses libgit2 `head().shorthand()` | After jj commit | Native |

### main.rs (doctor command)
| Binary | Call | When | Status |
|--------|------|------|--------|
| `pkl` | `pkl --version` | Doctor diagnostics | **GUARDED** |
| `git` | `git --version` | Doctor diagnostics | **GUARDED** |
| `jj` | `jj version`, `jj log` | Doctor diagnostics | **GUARDED** |

### settings.rs (custom postures)
| Binary | Call | When | Status |
|--------|------|------|--------|
| `pkl` | `pkl --version` + `rpkl::from_config()` | Loading custom `.pkl` postures | **GUARDED** |

---

## omegon-git crate

### jj.rs
| Binary | Call | When | Status |
|--------|------|------|--------|
| `jj` | `jj new -m <desc>` | Create new change | **JJ-ONLY** |
| `jj` | `jj describe -m <desc>` | Set change description | **JJ-ONLY** |
| `jj` | `jj squash` | Squash change into parent | **JJ-ONLY** |
| `jj` | `jj bookmark set <name> -r <rev>` | Set branch bookmark | **JJ-ONLY** |
| `jj` | `jj diff --summary -r @` | List dirty files | **JJ-ONLY** |
| `jj` | `jj git export` | Sync jj state to git refs | **JJ-ONLY** |
| `jj` | `jj log --template commit_id` | Get parent commit SHA | **JJ-ONLY** |
| `git` | `git rev-parse refs/heads/main` | Read main branch SHA (jj sync) | **JJ-ONLY** |
| `git` | `git merge-base --is-ancestor` | Ancestry check (jj sync) | **JJ-ONLY** |
| `git` | `git branch -f main <sha>` | Fast-forward main (jj sync) | **JJ-ONLY** |
| `git` | `git branch --show-current` | Check current branch (jj sync) | **JJ-ONLY** |
| `git` | `git checkout main` | Reattach HEAD (jj sync) | **JJ-ONLY** |

### repo.rs
| Binary | Call | When | Status |
|--------|------|------|--------|
| `jj` | `jj log -r @ -T change_id` | Read change ID at init/refresh | **JJ-ONLY** |

### worktree.rs
| Binary | Call | When | Status |
|--------|------|------|--------|
| `jj` | `jj workspace add <path> --name <name>` | Create jj workspace | **JJ-ONLY** |
| `jj` | `jj workspace forget <name>` | Remove jj workspace | **JJ-ONLY** |
| `jj` | `jj workspace list` | List jj workspaces | **JJ-ONLY** |
| *(none)* | Git worktree create/remove/prune via libgit2 | Git fallback path | Native |

### commit.rs
| Binary | Call | When | Status |
|--------|------|------|--------|
| `git` | `git add <submodule_path>` | Staging submodule pointers | **REQUIRED** |
| `git` | Various (init, config, clone, etc.) | Test setup only | **TEST-ONLY** |

### submodule.rs
| Binary | Call | When | Status |
|--------|------|------|--------|
| `git` | `git submodule update --init --recursive` | Initialize submodules in worktrees | **REQUIRED** |

### merge.rs
| Binary | Call | When | Status |
|--------|------|------|--------|
| `git` | `git cherry-pick <oid>` | Cleave branch merge | **REQUIRED** |
| `git` | `git cherry-pick --abort` | Abort on conflict | **REQUIRED** |
| `git` | `git checkout -` | Return to previous branch | **REQUIRED** |

---

## omegon-codescan crate

### indexer.rs
| Binary | Call | When | Status |
|--------|------|------|--------|
| *(none)* | HEAD check uses libgit2 `Repository::discover()` | Index cache key | Native |

---

## Summary: Required External Binaries

| Binary | Required by | Can eliminate? |
|--------|-------------|---------------|
| **bash** | Shell execution tool | No — shell semantics are irreplaceable |
| **git** (subset) | Submodule staging, cherry-pick, jj sync | Partial — cherry-pick and submodule ops lack libgit2 equivalents |
| **jj** | All jj operations | No — CLI is the stable contract (see jj-lib decision record) |
| **pkl** | Custom posture configs | No — canonical config format; guarded with clear install message |

Everything else is optional with graceful degradation.
