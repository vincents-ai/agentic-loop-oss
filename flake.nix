{
  description = "agentic-loop-oss - Open-source AI agent loop with engram integration";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    advisory-db = {
      url = "github:rustsec/advisory-db";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, crane, flake-utils, rust-overlay, advisory-db, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ rust-overlay.overlays.default ];
        pkgs = import nixpkgs { inherit system overlays; };

        inherit (pkgs) lib;

        rustVersion = "1.95.0";
        rustToolchain = pkgs.rust-bin.stable.${rustVersion}.default;

        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        src = craneLib.cleanCargoSource ./.;

        commonArgs = {
          inherit src;
          strictDeps = true;

          buildInputs = with pkgs; [
            pkg-config
            openssl
            protobuf
            git
          ] ++ lib.optionals stdenv.isDarwin [
            libiconv
          ];
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;
      in
      {
        checks = {
          workspace-clippy = craneLib.cargoClippy (commonArgs // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets -- --deny warnings";
          });

          workspace-fmt = craneLib.cargoFmt { inherit src; };

          workspace-audit = craneLib.cargoAudit {
            inherit src advisory-db;
          };

          workspace-nextest = craneLib.cargoNextest (commonArgs // {
            inherit cargoArtifacts;
            partitions = 1;
            partitionType = "count";
            cargoNextestPartitionsExtraArgs = "--no-tests=pass";
          });
        };

        packages.default = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          cargoExtraArgs = "-p agentic-loop-bin";
        });

        devShells.default = craneLib.devShell {
          checks = self.checks.${system};

          packages = with pkgs; [
            cargo-hakari
            pkg-config
            openssl
            protobuf
            git
          ];

          ENGRAM_PATH = "/home/shift/code/agentic-git/engram";
          VINCENTS_LLM_PATH = "/home/shift/code/vincents-ai/llm";
          VINCENTS_LLM_WRAPPER_PATH = "/home/shift/code/vincents-ai/llm-wrapper";
          AGENTIC_CORE_PLUGIN_PATH = "/home/shift/code/vincents-ai/skynet/plugins/agentic-core-plugin";

          RUST_LOG = "info";
        };
      }
    );
}
