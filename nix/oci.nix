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

  # Entrypoint that bootstraps secrets from env vars, then runs omegon.
  entrypoint = pkgs.writeShellApplication {
    name = "omegon-entrypoint";
    runtimeInputs = [ pkgs.coreutils pkgs.bashInteractive ];
    text = ''
      OMEGON_HOME="''${OMEGON_HOME:-/data/omegon}"
      SECRETS_JSON="$OMEGON_HOME/secrets.json"
      mkdir -p "$OMEGON_HOME"

      # Bootstrap secrets from environment variables.
      # Writes env: recipes — omegon resolves them at runtime.
      if [ ! -f "$SECRETS_JSON" ]; then
        echo "{}" > "$SECRETS_JSON"
        chmod 600 "$SECRETS_JSON"
      fi

      # Build recipes JSON from well-known env vars
      RECIPES="{"
      FIRST=true
      for VAR in ANTHROPIC_API_KEY OPENAI_API_KEY OPENROUTER_API_KEY \
                 VOX_DISCORD_BOT_TOKEN VOX_SLACK_BOT_TOKEN VOX_SLACK_APP_TOKEN \
                 VOX_SIGNAL_PASSWORD VOX_EMAIL_PASSWORD VOX_MATRIX_PASSWORD \
                 VOX_LXMF_IDENTITY GITHUB_TOKEN; do
        VAL="$(printenv "$VAR" 2>/dev/null || true)"
        if [ -n "$VAL" ]; then
          if [ "$FIRST" = true ]; then FIRST=false; else RECIPES="$RECIPES,"; fi
          RECIPES="$RECIPES \"$VAR\": \"env:$VAR\""
        fi
      done
      RECIPES="$RECIPES }"
      echo "$RECIPES" > "$SECRETS_JSON"
      chmod 600 "$SECRETS_JSON"

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
