# omegon-full substrate seed

This directory is the Nex-facing seed for the full-first Omegon OCI subagent substrate.

Current status:

- packaging backend: `nix/oci.nix` via `nix2container`
- profile source: `profile.toml`
- package/deployment intent: `styrene-package.toml`
- validated local image tag: `ghcr.io/styrene-lab/omegon-full:0.27.0-local`

The intended future command is:

```bash
nex build-image substrates/omegon-full
```

Until Nex image materialization is wired in, build/export/load/smoke through the repository `just` recipes and the Nix OCI backend:

```bash
just oci-build-local oci-full
just oci-export-local oci-full ghcr.io/styrene-lab/omegon-full:0.27.0-local
just oci-load-local result-oci-full-aarch64-linux.tar
just oci-smoke ghcr.io/styrene-lab/omegon-full:0.27.0-local
```

On macOS without a trusted Linux builder, run the same Nix build/export steps inside the Lima Linux builder, copy the archive back to the host, then load and smoke it with Podman.

This seed is deliberately full-first. Role-specific images such as `omegon-coding-rust` are trim-down targets after the full substrate proves real delegate/cleave dogfood workflows.
