+++
id = "40fec531-de5d-41fb-aca5-da1b0df37ebe"
kind = "document"
title = "omegon.styrene.dev — installation landing page + install.sh hosting"
status = "implemented"
tags = ["infrastructure", "distribution", "web"]
aliases = ["omegon-install-site"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
+++

# omegon.styrene.dev — installation landing page + install.sh hosting

## Overview

Set up omegon.styrene.dev as the canonical installation endpoint for the omegon binary. The install script already references this URL (`curl -fsSL https://omegon.styrene.dev/install.sh | sh`) but the domain doesn't resolve yet.

## Research

### Existing Infrastructure

**DNS**: Cloudflare-managed via external-dns in brutus k3s cluster. The `external-dns-styrene` app syncs DNS for both `styrene.io` and `styrene.dev` domains. Records created from Gateway API HTTPRoute annotations (`external-dns.alpha.kubernetes.io/cloudflare: "true"`). Default target: `216.82.35.160`.

**TLS**: cert-manager with Let's Encrypt via Cloudflare DNS-01 challenge. Both `styrene.io` and `styrene.dev` wildcards are already configured in the ClusterIssuer. Cloudflare API token stored in Vault → synced to k8s secret.

**Gateway**: Envoy Gateway with per-hostname listeners on the `vanderlyn` Gateway resource. Existing `styrene.dev` listener routes to `styrene-docs-staging` service. `docs.styrene.dev` redirects 301 to `styrene.dev/docs/`. Pattern for adding a new subdomain: add a listener section + HTTPRoute + external-dns annotation.

**Existing styrene-docs**: nginx container serving static files from GHCR image, deployed in `styrene-docs` namespace. Production (`styrene.io`) and staging (`styrene.dev`) deployments with separate services.

**Release artifacts**: GitHub Releases on `styrene-lab/omegon-core` with 4 platform tarballs (darwin-arm64, darwin-x64, linux-x64, linux-arm64). `install.sh` already fetches the latest release via the GitHub API.

### Architecture Options

**Option A: Static site in styrene-docs namespace**
Add an nginx container serving a minimal static site (landing page + install.sh). Reuse the same deployment pattern as styrene-docs (GHCR image, namespace, service). New HTTPRoute for `omegon.styrene.dev`. ~5 files in vanderlyn/apps.

Pros: Matches existing patterns exactly. Easy to maintain. Landing page can be a single HTML file.
Cons: Separate container image for a tiny static site. Needs CI to build/push the image.

**Option B: Path-based route on existing styrene-docs**
Add `omegon.styrene.dev` as a hostname alias on the existing styrene-docs deployment. Serve `/install.sh` and the landing page from a subdirectory in the styrene-docs repo.

Pros: No new container. No new deployment.
Cons: Couples omegon distribution to the docs site. install.sh updates require a styrene-docs rebuild.

**Option C: GitHub Pages with CNAME**
Point `omegon.styrene.dev` via Cloudflare DNS to GitHub Pages. Serve from a `gh-pages` branch on omegon-core or a dedicated repo. install.sh served directly from the repo.

Pros: Zero cluster resources. GitHub handles TLS. Free CDN. install.sh stays in the omegon-core repo.
Cons: Cloudflare proxying + GitHub Pages TLS can conflict. Requires careful CNAME setup. Less control over headers.

**Option D: Cloudflare Pages (serverless)**
Deploy a static site to Cloudflare Pages with `omegon.styrene.dev` as custom domain. Zero-origin, edge-served.

Pros: Fastest possible. Global CDN. Zero cluster load. Easy CI (push to repo → deploy).
Cons: Another service to manage. Cloudflare lock-in (mild — it's just static files).

**Option E: Redirect + raw GitHub**
`omegon.styrene.dev/install.sh` → 302 redirect to `raw.githubusercontent.com/styrene-lab/omegon-core/main/install.sh`. Landing page at `omegon.styrene.dev/` served from the cluster (tiny static page).

Pros: install.sh always up to date from the repo. Minimal infrastructure.
Cons: Redirect adds latency. GitHub rate limits on raw.githubusercontent.com for unauthenticated requests.

**Recommendation**: Option A is the cleanest. It follows the existing pattern exactly, keeps the install.sh and landing page under version control in their own image, and the landing page can be a beautiful single-page site with the Alpharius theme. Total cost: one tiny nginx pod (~32Mi RAM).

### Required Changes (Option A)

**1. vanderlyn/apps/envoy-gateway/gateway.yaml**
Add a listener section:
```yaml
- name: https-omegon-styrene-dev
  protocol: HTTPS
  port: 443
  hostname: omegon.styrene.dev
  allowedRoutes:
    namespaces:
      from: Selector
      selector:
        matchLabels:
          gateway/styrene: "true"
  tls:
    certificateRefs:
      - name: styrene-dev-tls
```

**2. vanderlyn/apps/envoy-gateway/httproutes-styrene.yaml**
Add HTTPRoute:
```yaml
apiVersion: gateway.networking.k8s.io/v1
kind: HTTPRoute
metadata:
  name: omegon-styrene-dev
  namespace: omegon-site
  annotations:
    external-dns.alpha.kubernetes.io/hostname: omegon.styrene.dev
    external-dns.alpha.kubernetes.io/cloudflare: "true"
spec:
  parentRefs:
    - name: vanderlyn
      namespace: envoy-gateway-system
      sectionName: https-omegon-styrene-dev
  hostnames:
    - omegon.styrene.dev
  rules:
    - backendRefs:
        - name: omegon-site
          port: 80
```

**3. New: vanderlyn/apps/omegon-site/**
- `namespace.yaml` — namespace `omegon-site` with label `gateway/styrene: "true"`
- `deployment.yaml` — nginx serving static files from GHCR image
- `service.yaml` — ClusterIP on port 80
- `vault-auth.yaml` — GHCR pull secret (reuse pattern from styrene-docs)

**4. New: omegon-core repo or dedicated repo**
- `site/index.html` — Alpharius-themed landing page
- `site/install.sh` — copy of current install.sh
- `site/nginx.conf` — minimal config with correct MIME types + caching headers
- `Containerfile` — nginx:alpine + COPY site files
- `.github/workflows/site.yml` — build + push to GHCR on changes to `site/`

**5. DNS + TLS** (automated)
- external-dns creates `omegon.styrene.dev → 216.82.35.160` CNAME automatically from the HTTPRoute annotation
- cert-manager issues TLS cert automatically (styrene.dev wildcard already configured)

## Decisions

### Decision: Option A: dedicated nginx pod in brutus cluster

**Status:** decided
**Rationale:** Matches existing styrene-docs pattern exactly. DNS, TLS, and gateway infrastructure already proven. 32Mi RAM cost is negligible.

### Decision: Serve install.sh directly, pinned to image SHA

**Status:** decided
**Rationale:** install.sh baked into the container image at build time. Deployment references image by SHA digest, not :latest tag. Updates require explicit image bump in the deployment manifest — no drift. Script must be bulletproof: verify checksums, handle partial downloads, clean up on failure, validate extracted binary.

### Decision: Just the install script — binaries stay on GitHub Releases

**Status:** decided
**Rationale:** GitHub Releases provides CDN and bandwidth for tarballs. No need to mirror. install.sh fetches from GitHub Releases API.

## Open Questions

*No open questions.*
