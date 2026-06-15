# Subagent OCI substrate

Default `delegate` and `cleave` children inherit the parent runtime: same working directory, same workspace permissions, same credentials, and same tool boundary model. Scope text in a child prompt is orientation, not a sandbox.

Use the OCI substrate when a subagent needs stronger isolation, a reproducible toolchain, or a role-specific runtime.

## Canonical runtime

Podman is the canonical local runtime. Docker-compatible commands are provided for hosts where Docker is the available engine.

The canonical development substrate starts from `ghcr.io/styrene-lab/omegon-full`. Smaller role images are optimization targets after the full workflow is proven.

Published image families are produced by the Nix OCI definitions in `nix/oci.nix` and `nix/profiles.nix`:

| Image | Intended use |
| --- | --- |
| `ghcr.io/styrene-lab/omegon-chat` | conversation-only agent |
| `ghcr.io/styrene-lab/omegon` | generic coding agent: shell, git, search, HTTP/archive tools |
| `ghcr.io/styrene-lab/omegon-coding-python` | coding agent with Python runtime |
| `ghcr.io/styrene-lab/omegon-coding-node` | coding agent with Node.js runtime |
| `ghcr.io/styrene-lab/omegon-coding-rust` | coding agent with Rust toolchain |
| `ghcr.io/styrene-lab/omegon-infra` | infrastructure agent |
| `ghcr.io/styrene-lab/omegon-full` | full-stack substrate for first-pass dogfood and trim-down |

For this repository, prove `omegon-full` first. Trim down to `omegon-coding-rust` only after the full substrate proves the Rust workflow and the smaller role image remains sufficient.

## Full-first substrate and trim-down strategy

Most subagent capability differences are runtime policy, not image identity:

- enabled/disabled Omegon tools;
- mounted credentials/config/state;
- network policy;
- writable workspace policy;
- extension availability;
- prompt/profile guidance.

The practical layering is:

1. **Image/toolchain layer** — executables and system libraries: `omegon`, shell, git, just, rg/fd, jq/curl, Python, Node, Rust, kubectl/helm/ssh where needed.
2. **Extension/Armory layer** — packaged extension bundles and their declared prerequisites. The extension SDK already forces prerequisites/dependencies to be declared; image builders should be able to compose those declarations into a layer.
3. **Mount layer** — user/project state: workspace, `OMEGON_HOME`, provider auth directories, caches, git/ssh/kube config when explicitly requested.
4. **Runtime capability layer** — enabled/disabled Omegon tools, network policy, write policy, profile-specific prompt guidance.

Do not bake user credentials or per-project state into images. Do bake executables and extension prerequisites that must be on `PATH`.

## Armory and extension-packaging anchor points

Armory should become the packaging guide for image layers. The target shape is not “one hand-written container per extension”; it is “compose image layers from declared extension prerequisites.”

Example task:

```text
do some research and then post the final report .md as an attachment to me in Discord
```

That requires at least three layers:

- base Omegon runtime (`omegon-full` while developing);
- research capability policy and tools (search/browser/HTTP tools as allowed by operator config);
- Vox extension availability plus Discord credentials/config mounted or injected.

The Vox extension should not be a bespoke exception. Its extension manifest/dependency declaration should let us derive:

- required external binaries, if any;
- required env vars/secrets;
- required readable/writable paths;
- optional network destinations;
- install location inside `OMEGON_HOME` or an extension bundle layer.

Initial image composition rule:

```text
full base image + armory extension layer + explicit mounts + runtime tool policy
```

Future image-builder rule:

```text
image = compose(base = omegon-full, extensions = [vox], profile = research-reporter)
```

This keeps the OCI substrate aligned with the extension SDK instead of duplicating dependency declarations in container files.

## Mount matrix

| Resource | Default | When to mount | Mode |
| --- | --- | --- | --- |
| workspace | yes | all coding/research tasks needing files | `rw` |
| `~/.omegon` / `OMEGON_HOME` | yes for trusted local dogfood | config, skills, extensions, memory | prefer `ro`; use `rw` only for explicit state writes |
| project `.omegon` | optional | project-local state | task-dependent |
| `~/.codex` | optional | Codex/OpenAI CLI OAuth adoption | `ro` |
| `~/.config/gh` | optional | GitHub CLI auth | `ro` |
| `~/.gitconfig` | optional | commits/pushes needing identity | `ro` |
| `~/.ssh` | optional | git SSH or infra work | `ro` |
| `~/.kube/config` | optional | infra tasks | `ro` |
| cargo/npm/pip caches | optional | speed | `rw` |
| Docker/Podman socket | no | only nested container work | avoid by default |

Broad mounts like an entire home directory or writable credential directories should be avoided unless the operator explicitly chooses that tradeoff.

## Smoke tests

From the repository root, build a local image for the native Linux architecture first:

```bash
just oci-build-local
```

On Apple Silicon this targets `aarch64-linux`; on x86_64 hosts it targets `x86_64-linux`. Override explicitly when needed:

```bash
OCI_SYSTEM=x86_64-linux just oci-build-local
OCI_SYSTEM=aarch64-linux just oci-build-local
```

Export and load the local Nix image when testing before publication:

```bash
just oci-export-local oci-full ghcr.io/styrene-lab/omegon-full:0.27.0-local
just oci-load-local result-oci-full-aarch64-linux.tar
just oci-smoke ghcr.io/styrene-lab/omegon-full:0.27.0-local
```

Validated Apple Silicon/Lima flow on 2026-06-15:

```bash
# inside the Lima Linux builder
nix build .#oci-full --accept-flake-config \
  -o "$HOME/omegon-oci-build/result-oci-full-aarch64-linux"
nix build .#oci-full.copyTo --accept-flake-config \
  -o "$HOME/omegon-oci-build/copy-oci-full-aarch64-linux"
"$HOME/omegon-oci-build/copy-oci-full-aarch64-linux/bin/copy-to" \
  docker-archive:"$HOME/omegon-oci-build/omegon-full-aarch64-linux.tar":ghcr.io/styrene-lab/omegon-full:0.27.0-local

# on the macOS host
limactl copy nix-builder:/home/wilson.guest/omegon-oci-build/omegon-full-aarch64-linux.tar ./.tmp-oci/omegon-full-aarch64-linux.tar
podman load -i ./.tmp-oci/omegon-full-aarch64-linux.tar
OCI_RUNTIME=podman just oci-smoke ghcr.io/styrene-lab/omegon-full:0.27.0-local
```

The validated local image passed smoke with `omegon`, `git`, `just`, `rg`, `jq`, Python, Node.js, Rust/Cargo, `kubectl`, and Helm present. After publication, smoke the public image instead:

```bash
just oci-smoke ghcr.io/styrene-lab/omegon-full
```

Use `OCI_PLATFORM=linux/amd64` or `OCI_PLATFORM=linux/arm64` only when intentionally testing a non-native platform image.

Equivalent Podman command:

```bash
podman run --rm \
  -v "$PWD:/workspace:Z" \
  -v "$HOME/.omegon:/data/omegon:ro,Z" \
  -w /workspace \
  ghcr.io/styrene-lab/omegon-full \
  bash -lc 'omegon --version && git --version && just --version && rg --version && jq --version && python --version && node --version && rustc --version && cargo --version && kubectl version --client=true && helm version --short'
```

Docker equivalent:

```bash
docker run --rm \
  -v "$PWD:/workspace" \
  -v "$HOME/.omegon:/data/omegon:ro" \
  -w /workspace \
  ghcr.io/styrene-lab/omegon-full \
  bash -lc 'omegon --version && git --version && just --version && rg --version && jq --version && python --version && node --version && rustc --version && cargo --version && kubectl version --client=true && helm version --short'
```

Notes:

- Podman on SELinux hosts should use `:Z` or `:z` volume labels.
- Docker does not use SELinux relabel suffixes on most macOS/Linux setups.
- Mount `/data/omegon` for Omegon config because the OCI image sets `OMEGON_HOME=/data/omegon`. The source entrypoint now only writes `secrets.json` when secret environment variables are present, so read-only config mounts work for smoke/config-only runs after the image is rebuilt. Use `OCI_OMEGON_HOME_RW=1 just oci-smoke` when intentionally testing secret bootstrap writes.
- Mount provider-specific external auth only when needed, e.g. `~/.codex` for Codex-compatible credentials.
- For isolated runs, mount credentials read-only and prefer short-lived tokens.

## Dogfooding Omegon in OCI

Dogfooding should begin with non-interactive and daemon surfaces before trying the full Ratatui TUI.

### Phase A — non-interactive smoke

```bash
just oci-smoke ghcr.io/styrene-lab/omegon-full
```

This proves the image has the expected binaries and the mount contract works.

### Phase B — daemon/control surface

Run the containerized agent as a daemon/service:

```bash
podman run --rm -it \
  -v "$PWD:/workspace:Z" \
  -v "$HOME/.omegon:/data/omegon:ro,Z" \
  -w /workspace \
  -p 7842:7842 \
  ghcr.io/styrene-lab/omegon-full \
  serve --control-port 7842
```

Docker equivalent drops the SELinux suffixes.

This is the best first dogfood path because it avoids terminal-emulation ambiguity.

Validated daemon probe on 2026-06-15 against `ghcr.io/styrene-lab/omegon-full:0.27.0-local`:

```bash
podman run -d --rm \
  -v "$PWD:/workspace" \
  -v "$HOME/.omegon:/data/omegon:ro" \
  -w /workspace \
  -p 127.0.0.1::7842 \
  ghcr.io/styrene-lab/omegon-full:0.27.0-local \
  serve --control-port 7842
```

Observed result:

- container stayed up;
- Podman mapped the control port to a host loopback port;
- a TCP probe reached the mapped control port;
- startup emitted the JSON startup record with health/ready/WebSocket URLs;
- read-only `/data/omegon` did not block daemon startup.

Known warnings from the validated local run:

- provider credentials were unavailable because only `~/.omegon` was mounted and Codex/OpenAI auth was not mounted into the path this image expected;
- host-built native extensions under `~/.omegon/extensions` failed with `Exec format error` on Linux/aarch64, confirming extension binaries need an Armory/image layer or architecture-specific install rather than a blind host extension mount;
- the IPC server reported `Operation not supported (os error 95)` under the container runtime, so daemon/control dogfood should use HTTP/WebSocket control paths first.

### Phase C — Ratatui/TUI in a container

Ratatui can work in a container if the process has a real TTY and reasonable terminal metadata. Use `-it`, pass `TERM`, and expect host-terminal features to degrade before core text rendering does.

Expected constraints:

- `-it` is required; without a TTY, Ratatui should not be the primary interface.
- Terminal size comes from the outer PTY; resize propagation usually works but should be smoke-tested.
- Clipboard integration, image protocols, and host-specific terminal features may not work or may need explicit mounts/sockets.
- Inline image rendering is likely the first Ratatui feature to degrade under containerization.
- Shelling out to host-specific tools only works if those tools exist in the image or are mounted explicitly.

Therefore the containerized TUI is useful for dogfood, but the production subagent substrate should prefer daemon/headless control paths unless the operator explicitly wants an interactive container session.

## Subagent contract

Initial substrate layering is deliberately simple:

1. inherited local runtime remains default for `delegate` and `cleave`;
2. OCI execution is explicit and operator-selected;
3. full image proves the workflow first;
4. role-specific images are trim-down optimizations;
5. future `delegate`/`cleave` execution modes can target an image once the image smoke tests are reliable.

A future execution-mode flag should look like:

```text
execution_mode = inherited | oci:<image>
```

Do not treat prompt scope as a security boundary. Use the OCI substrate when filesystem, network, dependency, or credential isolation matters.

## Nex dogfood package seed

The first Nex-facing substrate seed lives at:

```text
substrates/omegon-full/profile.toml
substrates/omegon-full/styrene-package.toml
```

This is deliberately a seed, not a second image builder. It captures the desired Nex package/profile intent while `nix/oci.nix` remains the packaging backend.

The intended future command is:

```bash
nex build-image substrates/omegon-full
```

Until Nex image materialization is wired into this repo, validate with:

```bash
just oci-smoke ghcr.io/styrene-lab/omegon-full
```

Mapping rule for the adapter:

```text
substrates/*/profile.toml capabilities/tools
  -> nix/profiles.nix domain/toolsets
  -> nix/oci.nix image output
```

The package file carries agent/deployment intent (`role`, `mode`, `posture`, image name/tag/ports). The profile file carries runtime capability intent (tools, mounts, secrets contracts, and boundary policy). Secret values remain runtime-only.
