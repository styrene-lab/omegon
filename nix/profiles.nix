# Omegon container toolset profiles.
#
# Profiles are organized into two tiers:
#
#   1. Foundations — atomic package sets that are never deployed alone.
#      These are building blocks composed into domains.
#
#   2. Domains — deployable agent roles. Each domain targets a specific
#      operational purpose and composes the foundations it needs.
#      One domain = one OCI image = one kind of agent Auspex can spawn.
#
# The agent's `bash` tool can only execute what's in PATH. A minimal
# container = a constrained agent. This is a security feature.

{ pkgs }:

let
  # ── Foundations ────────────────────────────────────────────────────────
  # Not deployed directly. Composed into domains below.

  shell = {
    name = "shell";
    packages = with pkgs; [
      bashInteractive
      coreutils
      cacert
      findutils
      gnugrep
      gnused
      gawk
      less
      which
      iptables  # required for filtered egress network policy
    ];
  };

  vcs = {
    name = "vcs";
    packages = with pkgs; [
      gitMinimal
      diffutils
      patch
    ];
  };

  search = {
    name = "search";
    packages = with pkgs; [
      ripgrep
      fd
      tree
      file
    ];
  };

  http = {
    name = "http";
    packages = with pkgs; [
      curl
      jq
    ];
  };

  archive = {
    name = "archive";
    packages = with pkgs; [
      gnutar
      gzip
    ];
  };

  python-runtime = {
    name = "python-runtime";
    packages = with pkgs; [
      python312
      python312Packages.pip
      python312Packages.virtualenv
    ];
  };

  node-runtime = {
    name = "node-runtime";
    packages = with pkgs; [
      nodejs_22
    ];
  };

  rust-runtime = {
    name = "rust-runtime";
    packages = with pkgs; [
      rustc
      cargo
      clippy
      rustfmt
    ];
  };

  k8s-tools = {
    name = "k8s-tools";
    packages = with pkgs; [
      kubectl
      kubernetes-helm
      k9s
      yq-go
    ];
  };

  ssh-tools = {
    name = "ssh-tools";
    packages = with pkgs; [
      openssh
    ];
  };

  net-diag = {
    name = "net-diag";
    packages = with pkgs; [
      iputils
      iproute2
      dnsutils
      nmap
      tcpdump
    ];
  };

in
{
  # Export foundations for custom compositions
  inherit shell vcs search http archive
          python-runtime node-runtime rust-runtime
          k8s-tools ssh-tools net-diag;

  # ── Domains ───────────────────────────────────────────────────────────
  # Each domain is a deployable agent role. Auspex picks the domain that
  # matches the task, spawns the corresponding image.

  # Conversational agent. No filesystem tools, no network. Pure LLM
  # reasoning — summarization, analysis, Q&A, triage.
  chat = {
    name = "chat";
    description = "Conversational agent. No dev tools, no network.";
    toolsets = [ shell ];
  };

  # Software engineering agent. Reads and writes code, runs git, searches
  # codebases. The default for most coding tasks.
  coding = {
    name = "coding";
    description = "Software engineering agent. Git, search, HTTP.";
    toolsets = [ shell vcs search http archive ];
  };

  # Coding agent with Python runtime. Can run tests, execute scripts,
  # manage virtualenvs.
  coding-python = {
    name = "coding-python";
    description = "Python project agent. Coding tools + Python 3.12.";
    toolsets = [ shell vcs search http archive python-runtime ];
  };

  # Coding agent with Node.js runtime.
  coding-node = {
    name = "coding-node";
    description = "Node.js project agent. Coding tools + Node 22.";
    toolsets = [ shell vcs search http archive node-runtime ];
  };

  # Coding agent with Rust toolchain.
  coding-rust = {
    name = "coding-rust";
    description = "Rust project agent. Coding tools + Rust stable.";
    toolsets = [ shell vcs search http archive rust-runtime ];
  };

  # Infrastructure and deployment agent. Manages k8s clusters, runs
  # helm, SSH into nodes, diagnoses network issues.
  infra = {
    name = "infra";
    description = "Infrastructure agent. kubectl, helm, SSH, network diag.";
    toolsets = [ shell vcs http archive k8s-tools ssh-tools net-diag ];
  };

  # Full-stack agent. Everything. Heavy image (~500MB), use sparingly.
  full = {
    name = "full";
    description = "Full-stack agent. All tools, all runtimes.";
    toolsets = [ shell vcs search http archive
                 python-runtime node-runtime rust-runtime
                 k8s-tools ssh-tools net-diag ];
  };
}
