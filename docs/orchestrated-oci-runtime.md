# Orchestrated OCI runtime

## Status

Seed design for Kubernetes/CRI-style deployments where Omegon is already running inside a container started by an external orchestrator.

This is deliberately separate from `omegon --oci`.

- `omegon --oci`: native host shim launches and supervises the OCI container.
- orchestrated OCI: Kubernetes/CRI/Nomad/ECS/systemd launches the container; Omegon discovers the runtime contract after startup.

## Runtime contexts

Omegon should distinguish at least four contexts:

| Context | Launcher | Owner of image/mount/port policy | Omegon responsibility |
| --- | --- | --- | --- |
| `inherited-host` | user shell/native launcher | host/operator | normal native operation |
| `host-shim-oci` | native `omegon --oci` | Omegon host shim | construct container run, strip OCI flags, supervise exit |
| `orchestrated-container` | Kubernetes/CRI/Nomad/ECS/etc. | orchestrator | introspect mounted contract, expose readiness/control surfaces |
| `manual-container` | direct `podman run`/`docker run` | operator/runtime command | behave as inner Omegon with diagnostics |

Being inside a container is not enough to infer that Omegon owns the launch lifecycle.

## Explicit environment contract

Heuristics are useful for diagnostics, but orchestrated deployments should set explicit environment variables:

```text
OMEGON_RUNTIME_CONTEXT=orchestrated-container
OMEGON_ORCHESTRATOR=kubernetes
OMEGON_INSIDE_OCI=1
OMEGON_WORKSPACE=/workspace
OMEGON_HOME=/data/omegon
OMEGON_CONTROL_PORT=7842
OMEGON_EGRESS_MODE=external
OMEGON_EXTENSION_POLICY=image-only
```

`omegon --oci` should instead set:

```text
OMEGON_RUNTIME_CONTEXT=host-shim-oci
OMEGON_OCI_LAUNCHER=omegon
OMEGON_INSIDE_OCI=1
```

If `--oci` is requested while `OMEGON_INSIDE_OCI=1`, Omegon should refuse recursive launch with a clear error.

## Detection fallback

When the explicit contract is absent, runtime diagnostics can infer context from:

- generic container files: `/.dockerenv`, `/run/.containerenv`;
- cgroup/container markers in `/proc/1/cgroup`;
- Kubernetes env: `KUBERNETES_SERVICE_HOST`, `KUBERNETES_SERVICE_PORT`;
- Kubernetes service account files under `/var/run/secrets/kubernetes.io/serviceaccount/`;
- container runtime env such as `container=podman|docker|containerd`.

Inferred context should be reported as diagnostic evidence, not treated as an immutable security boundary.

## Startup behavior

In orchestrated mode, Omegon must not try to perform host-shim duties:

- do not run `podman` or `docker`;
- do not pull images;
- do not map host ports;
- do not assume a desktop `~/.omegon`;
- do not assume a native host binary exists outside the container;
- do not mount or inspect host paths beyond the mounted contract.

The image should default to daemon mode:

```bash
omegon serve --control-port ${OMEGON_CONTROL_PORT:-7842}
```

The entrypoint should respect orchestrator-provided env and volumes, then exec Omegon.

## Home, workspace, and config separation

For real orchestrated deployment, workspace and home/config must be separate:

```text
HOME=/data/home
OMEGON_HOME=/data/omegon
WORKSPACE=/workspace
```

Recommended mounts:

| Purpose | Path | Typical source |
| --- | --- | --- |
| workspace | `/workspace` | `emptyDir`, PVC, git init container, CSI volume |
| Omegon state/config | `/data/omegon` | ConfigMap/Secret/PVC/emptyDir depending on persistence |
| home/config cache | `/data/home` | emptyDir or projected config |
| secrets | `/var/run/secrets/omegon/*` | Kubernetes Secret / External Secrets / CSI driver |

The previous smoke image used `HOME=/workspace`, which is acceptable for basic smoke but causes provider auth probes under the workspace. Orchestrated mode should not use workspace as home.

## Readiness and liveness

The daemon/control surface should be the primary orchestrated interface.

Stable endpoints:

```text
GET /api/healthz
GET /api/readyz
GET /api/startup
```

Recommended semantics:

- `healthz`: process/event loop is alive;
- `readyz`: control plane is accepting requests;
- capability/provider/auth status: reported separately, not automatically readiness-fatal.

Missing provider auth should not fail readiness unless the deployment explicitly requires auth readiness:

```text
OMEGON_REQUIRED_PROVIDER=openai-codex
OMEGON_REQUIRE_AUTH_READY=1
```

## Secrets

Kubernetes should not rely on host `~/.omegon`.

Supported/preferred patterns:

1. Environment variables from Secrets. Existing entrypoint recipe generation can translate known env vars into `secrets.json` recipes.
2. Mounted secret files, preferred for reduced env leakage:

```text
/var/run/secrets/omegon/openai-api-key
```

Desired recipe shape:

```json
{
  "OPENAI_API_KEY": "file:/var/run/secrets/omegon/openai-api-key"
}
```

3. External Secrets/Vault/CSI drivers projected as files.

## Network policy

`omegon --oci` can use host-shim iptables when the container has the required capability. Kubernetes should prefer CNI-native enforcement:

```text
OMEGON_EGRESS_MODE=external
```

Apply Kubernetes `NetworkPolicy` or Cilium/Calico equivalents outside the container. Do not request `NET_ADMIN` in cluster deployments unless explicitly required.

## Extension policy

Do not assume host-mounted native extensions work in orchestrated Linux containers. The validated local daemon run showed host extension binaries can fail with `Exec format error`.

Default orchestrated policy:

```text
OMEGON_EXTENSION_POLICY=image-only
```

Viable extension sources:

- baked image layer;
- init container installing architecture-compatible extensions into a shared volume;
- sidecar extension services over MCP/HTTP;
- future Armory/Nex image materialization.

Blind mounts of developer `~/.omegon/extensions` are not part of the Kubernetes contract.

## Startup diagnostics

At startup, containerized Omegon should emit a compact runtime report, for example:

```json
{
  "runtime_context": "orchestrated-container",
  "orchestrator": "kubernetes",
  "workspace": "/workspace",
  "omegon_home": "/data/omegon",
  "home": "/data/home",
  "control_port": 7842,
  "extension_policy": "image-only",
  "egress_mode": "external",
  "service_account": true,
  "namespace": "agents",
  "pod_name": "omegon-worker-...",
  "node_name": "..."
}
```

This distinguishes host-shim, manual container, CI container, and CRI deployment failures.

## Minimal Kubernetes skeleton

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: omegon-agent
spec:
  replicas: 1
  selector:
    matchLabels:
      app: omegon-agent
  template:
    metadata:
      labels:
        app: omegon-agent
    spec:
      containers:
        - name: omegon
          image: ghcr.io/styrene-lab/omegon-full:0.27.0
          args: ["serve", "--control-port", "7842"]
          env:
            - name: OMEGON_RUNTIME_CONTEXT
              value: orchestrated-container
            - name: OMEGON_ORCHESTRATOR
              value: kubernetes
            - name: OMEGON_INSIDE_OCI
              value: "1"
            - name: OMEGON_EGRESS_MODE
              value: external
            - name: OMEGON_EXTENSION_POLICY
              value: image-only
            - name: HOME
              value: /data/home
            - name: OMEGON_HOME
              value: /data/omegon
          ports:
            - name: control
              containerPort: 7842
          volumeMounts:
            - name: workspace
              mountPath: /workspace
            - name: omegon-home
              mountPath: /data/omegon
            - name: omegon-home-dir
              mountPath: /data/home
          readinessProbe:
            httpGet:
              path: /api/readyz
              port: control
          livenessProbe:
            httpGet:
              path: /api/healthz
              port: control
      volumes:
        - name: workspace
          emptyDir: {}
        - name: omegon-home
          emptyDir: {}
        - name: omegon-home-dir
          emptyDir: {}
```

This is a dogfood skeleton, not a production security profile. Production manifests should add resource requests/limits, service account policy, network policy, and explicit secret/config mounts.

## Integration with the OCI CLI roadmap

Add this as a distinct phase after the host-shim work:

### Phase 5 — Orchestrated OCI runtime

- Define and honor `OMEGON_RUNTIME_CONTEXT`.
- Add container/orchestrator detection diagnostics.
- Separate `HOME` from workspace in image/run defaults.
- Add Kubernetes manifest skeletons.
- Define readiness semantics independent of provider availability.
- Prefer `OMEGON_EGRESS_MODE=external` for CRI deployments.
- Disable host extension assumptions by default.

## Open questions

- Should orchestrated mode be explicit-only, or should Kubernetes env auto-select it when no explicit context is set?
- What exact provider auth file paths should be supported after `HOME=/data/home`?
- Should readiness ever fail on missing provider auth by default, or only when `OMEGON_REQUIRE_AUTH_READY=1` is set?
- Should the first deploy artifact be plain Kubernetes manifests or a Helm chart?
