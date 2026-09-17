{
  description = "Pure Rust AHX decoder and development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      fenix,
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
      manifest = builtins.fromTOML (builtins.readFile ./Cargo.toml);
      toolchains = forAllSystems (
        system:
        fenix.packages.${system}.stable.withComponents [
          "cargo"
          "rustc"
          "rust-src"
          "rustfmt"
          "clippy"
          "rust-analyzer"
        ]
      );
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          rustPlatform = pkgs.makeRustPlatform {
            cargo = toolchains.${system};
            rustc = toolchains.${system};
          };
        in
        {
          default = rustPlatform.buildRustPackage {
            pname = manifest.package.name;
            version = manifest.package.version;
            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;
            meta = {
              description = "Safe allocation-free CRI AHX decoder";
              license = pkgs.lib.licenses.lgpl21Plus;
              mainProgram = "ahx";
              platforms = systems;
            };
          };
        }
      );
      checks = forAllSystems (system: {
        package = self.packages.${system}.default;
        portable = self.packages.${system}.default.overrideAttrs {
          name = "ahx-portable";
          checkPhase = ''
            runHook preCheck
            cargo test --locked --offline
            cargo test --locked --offline --no-default-features
            cargo test --locked --offline --release --no-default-features
            cargo build --locked --offline --manifest-path tests/no-std/Cargo.toml --release
            runHook postCheck
          '';
          installPhase = ''touch "$out"'';
        };
      });
      devShells = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          releaseTools = with pkgs; [
            git
            gh
            jq
            curl
          ];
        in
        {
          default = pkgs.mkShell {
            packages = [
              toolchains.${system}
            ]
            ++ releaseTools
            ++ (with pkgs; [
              clang
              actionlint
              shellcheck
              taplo
              ruff
              nixfmt
              shfmt
            ]);
            RUST_SRC_PATH = "${toolchains.${system}}/lib/rustlib/src/rust/library";
          };
          release = pkgs.mkShellNoCC { packages = releaseTools; };
        }
      );
    };
}
