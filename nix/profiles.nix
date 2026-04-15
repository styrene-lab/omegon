# Omegon container toolset profiles.
#
# Each profile defines the binary surface available to the agent inside
# the container. The agent's `bash` tool can only execute what's in PATH.
# A minimal container = a constrained agent. This is a security feature.
#
# Profiles compose via layers — each adds packages without removing any
# from the base. Advanced operators combine profiles for their use case.
#
# Usage from flake.nix:
#   profiles = import ./nix/profiles.nix { inherit pkgs; };
#   omegonImage = mkOmegonImage { toolsets = [ profiles.base profiles.dev ]; };

{ pkgs }:

{
  # ── Base ─────────────────────────────────────────────────────────────────
  # Absolute minimum for omegon to function. The agent can run shell
  # commands but has almost no tools. Suitable for pure LLM tasks
  # (conversation, analysis) where file/network access is not needed.
  base = {
    name = "base";
    description = "Minimal shell + TLS. No dev tools.";
    packages = with pkgs; [
      bashInteractive
      coreutils
      cacert
      findutils        # find, xargs
      gnugrep          # grep
      gnused           # sed
      gawk             # awk
      less
      which
    ];
  };

  # ── Dev ──────────────────────────────────────────────────────────────────
  # Standard development tools. The agent can navigate codebases, search
  # files, make commits, and interact with HTTP APIs. This is the default
  # profile for coding agents.
  dev = {
    name = "dev";
    description = "Git, search, HTTP. Standard coding agent.";
    packages = with pkgs; [
      gitMinimal
      curl
      jq
      tree
      ripgrep
      fd
      diffutils
      patch
      file
      gnutar
      gzip
    ];
  };

  # ── Python ───────────────────────────────────────────────────────────────
  # Python runtime for agents that need to run or analyze Python code.
  python = {
    name = "python";
    description = "Python 3.12 + pip + venv.";
    packages = with pkgs; [
      python312
      python312Packages.pip
      python312Packages.virtualenv
    ];
  };

  # ── Node ─────────────────────────────────────────────────────────────────
  # Node.js runtime for agents working with JavaScript/TypeScript projects.
  node = {
    name = "node";
    description = "Node.js 22 LTS + npm.";
    packages = with pkgs; [
      nodejs_22
    ];
  };

  # ── Rust ─────────────────────────────────────────────────────────────────
  # Rust toolchain for agents working on Rust projects.
  rust = {
    name = "rust";
    description = "Rust stable toolchain + cargo.";
    packages = with pkgs; [
      rustc
      cargo
      clippy
      rustfmt
    ];
  };

  # ── Ops ──────────────────────────────────────────────────────────────────
  # Operations/infrastructure tools for agents managing deployments.
  ops = {
    name = "ops";
    description = "kubectl, helm, ssh, ops tooling.";
    packages = with pkgs; [
      openssh
      kubectl
      kubernetes-helm
      k9s
      yq-go
    ];
  };

  # ── Network ──────────────────────────────────────────────────────────────
  # Network diagnostic tools for agents troubleshooting connectivity.
  network = {
    name = "network";
    description = "DNS, ping, traceroute, nmap.";
    packages = with pkgs; [
      iputils        # ping
      iproute2       # ip, ss
      dnsutils       # dig, nslookup
      nmap
      tcpdump
    ];
  };
}
