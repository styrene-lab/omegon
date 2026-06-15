# OCI CLI execution boundary

## Status

Seed design after validating the `omegon-full` OCI substrate locally.

Validated substrate evidence:

- `oci-full` builds through Nix/nix2container for `aarch64-linux` in the Lima Linux builder.
- The resulting image exports to a Docker archive, loads into macOS Podman, and passes `just oci-smoke`.
- `ghcr.io/styrene-lab/omegon-full:0.27.0-local` starts `omegon serve --control-port 7842` in a container and exposes a reachable loopback control port.

## Intent

Add an imperative host-shim mode so a native `omegon` binary can launch the real Omegon process inside an OCI container:

```bash
omegon --oci
omegon --oci -p "fix this bug"
omegon --oci serve --control-port 7842
omegon --oci --oci-image ghcr.io/styrene-lab/omegon-full:0.27.0-local --version
```

The native binary remains the thin host integration layer. The container becomes the comprehensive runtime boundary for dependency closure, filesystem demarcation, architecture-specific extension layers, and future delegate/cleave execution modes.

## Existing implementation anchor

`core/crates/omegon/src/main.rs` already contains a `--sandboxed` path that re-execs Omegon inside an OCI container through `run_sandboxed`. This is the right conceptual anchor, but it is not yet the final `--oci` contract.

Current strengths:

- detects a container runtime;
- pulls an image when missing;
- mounts a workspace;
- sets an inside-container guard;
- applies sandbox posture such as read-only root, tmpfs, caps, resource limits, and egress filtering;
- forwards a subset of CLI options.

Current gaps relative to `--oci`:

- no user-facing `--oci` alias or OCI-specific flags;
- default image is `ghcr.io/styrene-lab/omegon:<version>`, while the validated substrate is `ghcr.io/styrene-lab/omegon-full:<tag>`;
- image override is environment-only (`OMEGON_SANDBOX_IMAGE`) rather than CLI-first;
- workspace is mounted at `/work`, while the validated OCI image and docs use `/workspace`;
- argument forwarding reconstructs selected fields from parsed `Cli` instead of preserving raw argv with OCI-only flags stripped;
- subcommands such as `serve` are not generally forwarded;
- fixed `-it` is not appropriate for all headless/daemon invocations;
- `serve` does not implement the validated dynamic host-port mapping strategy;
- config/auth/home separation is unresolved (`HOME=/workspace` caused provider auth probes under `/workspace/.config/...` in the daemon test);
- host-mounted native extensions can fail with `Exec format error`, so host extension autospawn must be disabled or replaced by image/Armory layers.

## Contract

`--oci` is a host-shim re-exec mode, not a separate agent mode.

```text
host omegon binary
  -> parse host-only OCI flags
  -> resolve runtime and image
  -> pull or verify image according to pull policy
  -> construct container mounts/env/ports
  -> strip OCI-only flags from raw argv
  -> run inner omegon with normal args inside container
  -> return container exit code
```

The inner process must receive normal Omegon arguments. It must not receive `--oci`, otherwise it can recursively launch another container.

Set an explicit guard:

```text
OMEGON_INSIDE_OCI=1
```

If the guard is set and the user requests `--oci`, fail closed with a clear recursion error.

## Proposed first CLI surface

Minimum MVP:

```text
--oci
--oci-image <IMAGE_REF>
--oci-runtime <podman|docker>
--oci-pull <missing|always|never>
```

Recommended aliases/relationship:

- `--oci` should be the concrete user-facing flag.
- Existing `--sandboxed` can become an alias or high-level synonym for OCI execution while OCI is the only sandbox backend.
- Longer term, expose an execution-boundary enum rather than accumulating booleans:

```text
--execution-boundary inherited|oci
```

## Image resolution

Default image policy should track release provenance:

- released host binary: `ghcr.io/styrene-lab/omegon-full:<CARGO_PKG_VERSION>`;
- dev/dirty host binary: require `--oci-image` or use an explicit development channel such as `nightly` with a warning;
- local dogfood: `--oci-image ghcr.io/styrene-lab/omegon-full:0.27.0-local`.

Do not silently run a mismatched public image for a dirty local host binary.

Pull policy:

| Policy | Behavior |
| --- | --- |
| `missing` | pull only when the image is absent locally |
| `always` | pull before every run |
| `never` | fail if absent locally |

Default should be `missing` for released host binaries and `never` for dev/local binaries unless `--oci-image` is explicit.

## Runtime resolution

Resolution order:

1. `--oci-runtime`;
2. `OMEGON_OCI_RUNTIME`;
3. `podman` if available;
4. `docker` if available;
5. fail with installation guidance.

Commands must use `std::process::Command` argument arrays. Do not shell-interpolate user-controlled args.

## Mount and environment policy

Initial safe default:

| Host resource | Container path | Mode | Notes |
| --- | --- | --- | --- |
| resolved workspace/cwd | `/workspace` | `rw` | coding requires writes by default |
| Omegon config/state | `/data/omegon` | `ro` | read-only unless explicit secret bootstrap/state write |
| container home | `/data/home` | `rw tmpfs` or managed volume | avoids config probes under workspace |
| host Codex/OpenAI auth | `/data/home/.codex` or provider-specific path | `ro`, opt-in | mount only when requested |
| Git config | `/data/home/.gitconfig` | `ro`, opt-in | commits/pushes only |
| SSH/Kube config | provider paths under `/data/home` | `ro`, opt-in | explicit infra use only |
| Docker/Podman socket | none | n/a | never by default |

The current image sets `HOME=/workspace`. That was sufficient for smoke, but a production `--oci` mode should separate home/config from workspace:

```text
HOME=/data/home
OMEGON_HOME=/data/omegon
WORKSPACE=/workspace
```

## Extension policy

The daemon probe showed host-mounted native extensions under `~/.omegon/extensions` can fail with `Exec format error` inside Linux/aarch64 containers.

Default `--oci` policy should therefore be one of:

```text
OMEGON_EXTENSION_POLICY=image-only
OMEGON_EXTENSION_POLICY=disabled
```

Host extensions should require an explicit escape hatch such as:

```bash
omegon --oci --oci-extensions host
```

with a warning that native binaries must match the container architecture.

Long-term, extension availability should come from Armory/Nex image layers, not blind host mounts.

## Argument forwarding

Preserve raw argv and strip only host-only OCI flags:

- `--oci`
- `--sandboxed` if treated as an OCI alias
- `--oci-image <value>` / `--oci-image=<value>`
- `--oci-runtime <value>` / `--oci-runtime=<value>`
- `--oci-pull <value>` / `--oci-pull=<value>`

Then remap cwd:

- resolve host cwd;
- mount host cwd or repo root at `/workspace`;
- forward `--cwd /workspace` or the equivalent repo-relative container path.

First pass can mount resolved `--cwd` directly at `/workspace`. A later pass should mount repo root and preserve subdirectory cwd as `/workspace/<relative>`.

## `serve` behavior

The validated daemon run showed fixed host port `7842` can be busy, while dynamic mapping works.

For:

```bash
omegon --oci serve --control-port 7842
```

recommended default:

```text
inner port: 7842
host port: runtime-assigned loopback port
```

Use fixed host port only when the user passes `--strict-port`.

The host shim should print the effective URLs after inspecting the container mapping:

```text
Omegon OCI daemon running:
  image:   ghcr.io/styrene-lab/omegon-full:0.27.0
  runtime: podman
  health:  http://127.0.0.1:<host_port>/api/healthz
  ready:   http://127.0.0.1:<host_port>/api/readyz
```

## Testing plan

Unit tests:

- OCI-only flag stripping preserves ordinary Omegon args and subcommands;
- `--oci` recursion guard fails when `OMEGON_INSIDE_OCI=1`;
- runtime detection order honors CLI/env/defaults;
- image resolution refuses unsafe dirty-build defaults;
- cwd remapping is deterministic;
- serve port mapping plan differs for strict vs non-strict mode.

Integration smoke:

```bash
omegon --oci --oci-image ghcr.io/styrene-lab/omegon-full:0.27.0-local --version
omegon --oci --oci-image ghcr.io/styrene-lab/omegon-full:0.27.0-local -p "smoke"
omegon --oci --oci-image ghcr.io/styrene-lab/omegon-full:0.27.0-local serve --control-port 7842
```

The prompt smoke should initially use a no-provider or deterministic mode if available; otherwise it requires explicit auth mounts.

## Phases

### Phase 1 — Shim MVP

- Add `--oci` plus `--oci-image`, `--oci-runtime`, and `--oci-pull`.
- Refactor existing `run_sandboxed` into a dedicated OCI launcher module.
- Use raw argv stripping instead of reconstructing selected CLI fields.
- Default to `omegon-full` image family.
- Support `--version`, headless, and `serve` forwarding.

### Phase 2 — Home/auth separation

- Change container run env to `HOME=/data/home` while preserving `OMEGON_HOME=/data/omegon` and workspace at `/workspace`.
- Add explicit auth mount modes.
- Document provider-specific auth adoption paths.

### Phase 3 — Extension/image layers

- Disable host native extension autospawn by default in OCI mode or support `image-only` extension policy.
- Compose Armory/Nex extension layers into image builds.

### Phase 4 — Publication and execution-mode integration

- Publish versioned and nightly GHCR images.
- Add CI smoke after pull.
- Wire delegate/cleave task execution mode to `oci:<image>` once host `--oci` is stable.

### Phase 5 — Orchestrated OCI runtime

Kubernetes/CRI-style deployments are not launched by the native `omegon --oci` host shim. The orchestrator owns image pull, mounts, ports, probes, service accounts, and network policy. Omegon should detect or be told that it is already running in an orchestrated container through `OMEGON_RUNTIME_CONTEXT=orchestrated-container`, then expose daemon readiness/control surfaces without attempting host-shim duties.

See `docs/orchestrated-oci-runtime.md` for the runtime contract, Kubernetes skeleton, readiness semantics, and extension/auth/network policy implications.

## Open questions

- Should `--sandboxed` remain a strict sandbox posture with read-only root and egress filtering, while `--oci` means containerized execution with configurable posture?
- Should the first production default image be `omegon-full` or the smaller generic `omegon` image after role images are smoke-proven?
- Should `--oci serve` keep the container attached in foreground or start detached and print connection info?
- What is the canonical provider-auth mount path after `HOME` moves to `/data/home`?
- Which extension policy env var should the runtime honor: `OMEGON_EXTENSION_POLICY=image-only|disabled|host` or a more general capability profile?
