# Omegon container image builder.
#
# Produces minimal OCI images per domain. Each domain is a deployable
# agent role (chat, coding, infra, etc.) with its own toolset surface.
#
# The image contains NO extension binaries. Extensions are installed
# via init-container sidecar pattern into a shared OMEGON_HOME volume.
#
# Domain images:
#   omegon-chat          → ~50MB   (conversational only)
#   omegon               → ~100MB  (coding agent, default)
#   omegon-coding-python → ~200MB  (Python project agent)
#   omegon-coding-node   → ~150MB  (Node.js project agent)
#   omegon-coding-rust   → ~300MB  (Rust project agent)
#   omegon-infra         → ~200MB  (infrastructure agent)
#   omegon-full          → ~500MB  (everything)

{ nix2container, pkgs, omegon, profiles, version, commitSha }:
let
  n2c = nix2container.packages.${pkgs.system}.nix2container;

  # Minimal root filesystem shared by all domains
  initDirs = pkgs.runCommand "omegon-container-init" {} ''
    mkdir -p $out/tmp $out/workspace $out/data/omegon $out/etc
    cat > $out/etc/passwd <<'PASSWD'
    root:x:0:0:root:/workspace:/bin/bash
    omegon:x:1000:1000:omegon:/workspace:/bin/bash
    nobody:x:65534:65534:nobody:/nonexistent:/usr/sbin/nologin
    PASSWD
    cat > $out/etc/group <<'GROUP'
    root:x:0:
    omegon:x:1000:
    nogroup:x:65534:
    GROUP
  '';

  # Entrypoint that bootstraps secrets from env vars, applies network
  # policy, then runs omegon.
  entrypoint = pkgs.writeShellApplication {
    name = "omegon-entrypoint";
    runtimeInputs = [ pkgs.coreutils pkgs.bashInteractive pkgs.jq ];
    text = ''
      OMEGON_HOME="''${OMEGON_HOME:-/data/omegon}"
      SECRETS_JSON="$OMEGON_HOME/secrets.json"

      # Let toolchain/debug smoke commands behave like ordinary OCI images.
      # Normal runs remain omegon-first via the final `exec omegon "$@"`.
      case "''${1:-}" in
        bash|sh|/bin/bash|/bin/sh)
          exec "$@"
          ;;
      esac

      # Bootstrap secrets from environment variables. Writes env: recipes —
      # omegon resolves them at runtime. If no known secret env vars are set,
      # do not write secrets.json; this lets read-only OMEGON_HOME mounts work
      # for smoke tests and config-only runs.
      RECIPES="{"
      FIRST=true
      for VAR in ANTHROPIC_API_KEY OPENAI_API_KEY OPENROUTER_API_KEY                  VOX_DISCORD_BOT_TOKEN VOX_SLACK_BOT_TOKEN VOX_SLACK_APP_TOKEN                  VOX_SIGNAL_PASSWORD VOX_EMAIL_PASSWORD VOX_MATRIX_PASSWORD                  VOX_LXMF_IDENTITY GITHUB_TOKEN; do
        VAL="$(printenv "$VAR" 2>/dev/null || true)"
        if [ -n "$VAL" ]; then
          if [ "$FIRST" = true ]; then FIRST=false; else RECIPES="$RECIPES,"; fi
          RECIPES="$RECIPES \\"$VAR\\": \\"env:$VAR\\""
        fi
      done
      RECIPES="$RECIPES }"
      if [ "$FIRST" = false ]; then
        mkdir -p "$OMEGON_HOME"
        echo "$RECIPES" > "$SECRETS_JSON"
        chmod 600 "$SECRETS_JSON"
      elif [ ! -e "$OMEGON_HOME" ]; then
        mkdir -p "$OMEGON_HOME"
      fi

      # ── Egress filter ──────────────────────────────────────────────
      # When OMEGON_EGRESS_FILTER is set (JSON), restrict outbound traffic.
      #
      # OMEGON_EGRESS_MODE controls the enforcement mechanism:
      #   iptables (default) — apply iptables rules (requires NET_ADMIN)
      #   external           — skip iptables; rely on cluster CNI
      #                        (Cilium/Calico NetworkPolicy, service mesh).
      #                        OMEGON_EGRESS_FILTER serves as documentation
      #                        of intent — apply the policy via k8s manifests.
      #   auto               — try iptables, fall back to external if it fails
      #
      # Standalone (podman/docker): mode=iptables + NET_ADMIN
      # Kubernetes (Cilium/Istio):  mode=external + CiliumNetworkPolicy
      EGRESS_MODE="''${OMEGON_EGRESS_MODE:-auto}"

      if [ -n "''${OMEGON_EGRESS_FILTER:-}" ]; then
        if [ "$EGRESS_MODE" = "external" ]; then
          echo "[nex] egress filter: external enforcement (CNI/NetworkPolicy)" >&2
          echo "[nex] OMEGON_EGRESS_FILTER is set for documentation — iptables skipped" >&2
        elif command -v iptables >/dev/null 2>&1; then
          # Try to apply iptables rules. If the first rule fails (no
          # NET_ADMIN, or eBPF-only network namespace), fall back to
          # external mode gracefully instead of leaving a half-applied
          # ruleset.
          if iptables -P OUTPUT DROP 2>/dev/null; then
            echo "[nex] applying egress filter via iptables" >&2

            # Allow loopback
            iptables -A OUTPUT -o lo -j ACCEPT

            # Allow established/related (return traffic)
            iptables -A OUTPUT -m state --state ESTABLISHED,RELATED -j ACCEPT

            # Allow DNS resolution (required for host-based filtering)
            iptables -A OUTPUT -p udp --dport 53 -j ACCEPT
            iptables -A OUTPUT -p tcp --dport 53 -j ACCEPT

            # Block cloud metadata endpoints if deny_metadata is true
            DENY_META="$(echo "$OMEGON_EGRESS_FILTER" | jq -r '.deny_metadata // true')"
            if [ "$DENY_META" = "true" ]; then
              iptables -A OUTPUT -d 169.254.169.254 -j DROP
              iptables -A OUTPUT -d fd00:ec2::254 -j DROP 2>/dev/null || true
            fi

            # Block RFC1918 private ranges if deny_private is true
            DENY_PRIV="$(echo "$OMEGON_EGRESS_FILTER" | jq -r '.deny_private // true')"
            if [ "$DENY_PRIV" = "true" ]; then
              iptables -A OUTPUT -d 10.0.0.0/8 -j DROP
              iptables -A OUTPUT -d 172.16.0.0/12 -j DROP
              iptables -A OUTPUT -d 192.168.0.0/16 -j DROP
            fi

            # Allow specific CIDRs
            for CIDR in $(echo "$OMEGON_EGRESS_FILTER" | jq -r '.allow_cidrs[]? // empty'); do
              PORTS="$(echo "$OMEGON_EGRESS_FILTER" | jq -r '.allow_ports[]? // empty')"
              if [ -z "$PORTS" ]; then
                iptables -A OUTPUT -d "$CIDR" -j ACCEPT
              else
                for PORT in $PORTS; do
                  iptables -A OUTPUT -d "$CIDR" -p tcp --dport "$PORT" -j ACCEPT
                  iptables -A OUTPUT -d "$CIDR" -p udp --dport "$PORT" -j ACCEPT
                done
              fi
            done

            # Resolve allowed hosts to IPs and add rules
            for HOST in $(echo "$OMEGON_EGRESS_FILTER" | jq -r '.allow_hosts[]? // empty'); do
              IPS="$(getent hosts "$HOST" 2>/dev/null | awk '{print $1}' || true)"
              if [ -z "$IPS" ]; then
                echo "[nex] warning: could not resolve $HOST — skipping" >&2
                continue
              fi
              PORTS="$(echo "$OMEGON_EGRESS_FILTER" | jq -r '.allow_ports[]? // empty')"
              for IP in $IPS; do
                if [ -z "$PORTS" ]; then
                  iptables -A OUTPUT -d "$IP" -j ACCEPT
                else
                  for PORT in $PORTS; do
                    iptables -A OUTPUT -d "$IP" -p tcp --dport "$PORT" -j ACCEPT
                    iptables -A OUTPUT -d "$IP" -p udp --dport "$PORT" -j ACCEPT
                  done
                fi
              done
            done

            echo "[nex] egress filter applied via iptables" >&2
          else
            echo "[nex] iptables failed (no NET_ADMIN or eBPF-only CNI)" >&2
            echo "[nex] falling back to external enforcement — apply NetworkPolicy via cluster CNI" >&2
            echo "[nex] generate policy: omegon nex networkpolicy" >&2
          fi
        else
          echo "[nex] iptables not found — egress filter requires external enforcement" >&2
          echo "[nex] generate policy: omegon nex networkpolicy" >&2
        fi
      fi

      exec omegon "$@"
    '';
  };

  # Build an OCI image from a domain definition.
  # Each foundation in the domain becomes its own layer for caching.
  mkOmegonImage = { name ? "omegon", tag ? version, domain }:
    let
      allPackages = builtins.concatMap (t: t.packages) domain.toolsets;
      foundationNames = builtins.map (t: t.name) domain.toolsets;
    in
    n2c.buildImage {
      name = "ghcr.io/styrene-lab/${name}";
      inherit tag;

      config = {
        entrypoint = [ "${entrypoint}/bin/omegon-entrypoint" ];
        cmd = [ "serve" "--control-port" "7842" ];
        env = [
          "OMEGON_HOME=/data/omegon"
          "HOME=/workspace"
          "RUST_LOG=info"
          "PATH=${pkgs.lib.makeBinPath (allPackages ++ [ omegon entrypoint ])}:/usr/local/bin"
          "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
          "NIX_SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
        ];
        exposedPorts = {
          "7842/tcp" = {};
        };
        workingDir = "/workspace";
        labels = {
          "org.opencontainers.image.version" = version;
          "org.opencontainers.image.revision" = commitSha;
          "org.opencontainers.image.title" = name;
          "org.opencontainers.image.description" = "Omegon agent — ${domain.description}";
          "org.opencontainers.image.source" = "https://github.com/styrene-lab/omegon";
          "sh.styrene.omegon.domain" = domain.name;
          "sh.styrene.omegon.foundations" = builtins.concatStringsSep "," foundationNames;
        };
      };

      copyToRoot = [ initDirs ];

      layers =
        # One layer per foundation for optimal caching
        builtins.map (foundation:
          n2c.buildLayer { deps = foundation.packages; }
        ) domain.toolsets
        # Final layer: omegon binary + entrypoint (changes on release)
        ++ [
          (n2c.buildLayer { deps = [ omegon entrypoint ]; })
        ];
    };
  # Build an OCI image from a domain + agent bundle directory.
  # The bundle is baked into $OMEGON_HOME/catalog/ so the agent starts
  # with --agent <id> and has everything it needs.
  mkAgentImage = { name, tag ? version, domain, bundlePath, agentId }:
    let
      bundleLayer = pkgs.runCommand "omegon-agent-bundle-${name}" {} ''
        mkdir -p $out/data/omegon/catalog
        cp -r ${bundlePath} $out/data/omegon/catalog/${agentId}
      '';
      base = mkOmegonImage { inherit name tag domain; };
    in
    n2c.buildImage {
      name = "ghcr.io/styrene-lab/${name}";
      inherit tag;
      fromImage = base;
      config = base.config // {
        cmd = [ "serve" "--control-port" "7842" "--agent" agentId ];
      };
      copyToRoot = [ bundleLayer ];
      layers = [
        (n2c.buildLayer { deps = [ bundleLayer ]; })
      ];
    };

in
{
  inherit mkOmegonImage mkAgentImage;

  # ── Domain images ───────────────────────────────────────────────────

  oci-chat = mkOmegonImage {
    name = "omegon-chat";
    domain = profiles.chat;
  };

  oci-coding = mkOmegonImage {
    name = "omegon";
    domain = profiles.coding;
  };

  oci-coding-python = mkOmegonImage {
    name = "omegon-coding-python";
    domain = profiles.coding-python;
  };

  oci-coding-node = mkOmegonImage {
    name = "omegon-coding-node";
    domain = profiles.coding-node;
  };

  oci-coding-rust = mkOmegonImage {
    name = "omegon-coding-rust";
    domain = profiles.coding-rust;
  };

  oci-infra = mkOmegonImage {
    name = "omegon-infra";
    domain = profiles.infra;
  };

  oci-full = mkOmegonImage {
    name = "omegon-full";
    domain = profiles.full;
  };
}
