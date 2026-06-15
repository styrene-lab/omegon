# Community headroom eval packs

Community packs cover languages and formats that are useful to someone, but not necessarily owned by Omegon core maintainers.

Examples:

- Java / JVM build logs
- C / C++ source and compiler output
- CSV / tabular text exports
- LaTeX / scientific documents
- Terraform / Kubernetes YAML

Rules:

- Keep sources public, legal, and bounded.
- Prefer generated fixtures under `.tmp/headroom/`; do not commit large corpora.
- Mark the pack owner/maintainer in `pack.toml`.
- Use `status = "community"` unless core explicitly adopts the pack.
- Do not set `required_for_release = true` or `required_for_default_on = true` for community packs.

Core maintainers may mark a community pack experimental or deprecated if it drifts and no maintainer is available.
