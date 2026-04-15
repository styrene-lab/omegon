{
  description = "Omegon — AI coding agent daemon and TUI";

  nixConfig = {
    extra-substituters      = [ "https://styrene.cachix.org" ];
    extra-trusted-public-keys = [
      "styrene.cachix.org-1:oyGX4VS45l/HvLNQvBHJ+PjIQ23mUI+XTzL8aOCvXUg="
    ];
  };

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.11";
    flake-utils.url = "github:numtide/flake-utils";

    crane = {
      url = "github:ipetkov/crane";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    nix2container = {
      url = "github:nlewo/nix2container";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, crane, nix2container }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        craneLib = crane.mkLib pkgs;

        workspaceVersion = "0.15.23";

        commitSha =
          if self ? shortRev then self.shortRev
          else if self ? dirtyShortRev then self.dirtyShortRev
          else "unknown";

        # Crane source filtering for the core workspace
        src = pkgs.lib.cleanSourceWith {
          src = craneLib.path ./core;
          filter = path: type:
            (craneLib.filterCargoSources path type)
            || builtins.match ".*\\.md$" path != null
            || builtins.match ".*\\.toml$" path != null;
        };

        commonArgs = {
          inherit src;
          pname = "omegon";
          strictDeps = true;
          buildInputs = with pkgs; [
            openssl
            sqlite
            pkg-config
          ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
            pkgs.darwin.apple_sdk.frameworks.Security
            pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
          ];
          nativeBuildInputs = with pkgs; [
            pkg-config
            cmake      # for libgit2-sys
          ];
          PKG_CONFIG_PATH = "${pkgs.openssl.dev}/lib/pkgconfig";
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        omegon = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          cargoExtraArgs = "-p omegon";
        });

        # Toolset profiles for container images
        profiles = import ./nix/profiles.nix { inherit pkgs; };

        # OCI images (Linux only)
        images = pkgs.lib.optionalAttrs pkgs.stdenv.isLinux (
          import ./nix/oci.nix {
            inherit nix2container pkgs omegon profiles commitSha;
            version = workspaceVersion;
          }
        );
      in
      {
        packages = {
          default = omegon;
          omegon = omegon;
        } // pkgs.lib.optionalAttrs pkgs.stdenv.isLinux {
          oci-base = images.oci-base;
          oci-dev = images.oci-dev;
          oci-python = images.oci-python;
          oci-node = images.oci-node;
          oci-full = images.oci-full;
          oci-ops = images.oci-ops;
        };

        # mkOmegonImage for custom compositions:
        #   nix build .#mkOmegonImage --override-input profiles '[base dev python]'
        lib = pkgs.lib.optionalAttrs pkgs.stdenv.isLinux {
          inherit (images) mkOmegonImage;
          inherit profiles;
        };

        devShells.default = craneLib.devShell {
          packages = with pkgs; [
            cargo-watch
            cargo-zigbuild
            just
            sqlite
          ];
        };
      }
    );
}
