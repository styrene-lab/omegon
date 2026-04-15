# Omegon container image builder.
#
# Produces minimal OCI images with composable toolset layers.
# The omegon binary is the only constant — everything else is
# determined by which profiles the operator selects.
#
# The image contains NO extension binaries. Extensions are installed
# via init-container sidecar pattern into a shared OMEGON_HOME volume.
#
# Example compositions:
#   base only          → 40-50MB  (conversation agent, no tools)
#   base + dev         → 80-100MB (coding agent, standard)
#   base + dev + python → 150MB   (Python project agent)
#   base + dev + ops   → 120MB   (infrastructure agent)

{ nix2container, pkgs, omegon, profiles, version, commitSha }:
let
  n2c = nix2container.packages.${pkgs.system}.nix2container;

  # Minimal root filesystem shared by all profiles
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

  # Build an OCI image from a list of profile sets.
  # Each profile becomes its own layer for optimal caching.
  mkOmegonImage = { name ? "omegon", tag ? version, toolsets ? [ profiles.base ] }:
    let
      allPackages = builtins.concatMap (p: p.packages) toolsets;
      profileNames = builtins.map (p: p.name) toolsets;
      profileDesc = builtins.concatStringsSep ", " profileNames;
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
          "org.opencontainers.image.description" = "Omegon agent daemon [${profileDesc}]";
          "org.opencontainers.image.source" = "https://github.com/styrene-lab/omegon";
          "sh.styrene.omegon.profiles" = builtins.concatStringsSep "," profileNames;
        };
      };

      copyToRoot = [ initDirs ];

      layers = [
        # Layer 1: base shell + coreutils (rarely changes)
        (n2c.buildLayer { deps = profiles.base.packages; })
      ]
      # Layer 2..N: each additional toolset profile
      ++ builtins.map (profile:
        n2c.buildLayer { deps = profile.packages; }
      ) (builtins.filter (p: p.name != "base") toolsets)
      # Final layer: omegon binary + entrypoint (changes on release)
      ++ [
        (n2c.buildLayer { deps = [ omegon entrypoint ]; })
      ];
    };
in
{
  # Pre-composed images for common use cases
  inherit mkOmegonImage;

  # Minimal — conversation-only agent, no dev tools
  oci-base = mkOmegonImage {
    name = "omegon-base";
    toolsets = [ profiles.base ];
  };

  # Standard coding agent — git, search, HTTP
  oci-dev = mkOmegonImage {
    name = "omegon";
    toolsets = [ profiles.base profiles.dev ];
  };

  # Python project agent
  oci-python = mkOmegonImage {
    name = "omegon-python";
    toolsets = [ profiles.base profiles.dev profiles.python ];
  };

  # Node.js project agent
  oci-node = mkOmegonImage {
    name = "omegon-node";
    toolsets = [ profiles.base profiles.dev profiles.node ];
  };

  # Full-stack agent (dev + python + node + rust)
  oci-full = mkOmegonImage {
    name = "omegon-full";
    toolsets = [ profiles.base profiles.dev profiles.python profiles.node profiles.rust ];
  };

  # Infrastructure / ops agent
  oci-ops = mkOmegonImage {
    name = "omegon-ops";
    toolsets = [ profiles.base profiles.dev profiles.ops profiles.network ];
  };
}
