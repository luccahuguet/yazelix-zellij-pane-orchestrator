{
  description = "Standalone Zellij pane orchestrator plugin from Yazelix";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      fenix,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
        };
        rustToolchain = fenix.packages.${system}.combine [
          fenix.packages.${system}.stable.cargo
          fenix.packages.${system}.stable.rustc
          fenix.packages.${system}.targets.wasm32-wasip1.stable.rust-std
        ];
        rustPlatform = pkgs.makeRustPlatform {
          cargo = rustToolchain;
          rustc = rustToolchain;
        };
        yazelixZellijPaneOrchestrator = rustPlatform.buildRustPackage {
          pname = "yazelix-zellij-pane-orchestrator";
          version = "0.1.0";
          src = pkgs.lib.cleanSource ./.;
          cargoLock.lockFile = ./Cargo.lock;
          doCheck = false;

          buildPhase = ''
            runHook preBuild

            cargo build \
              --target-dir target \
              --offline \
              --profile release \
              --target wasm32-wasip1

            runHook postBuild
          '';

          installPhase = ''
            runHook preInstall

            install -Dm644 \
              target/wasm32-wasip1/release/yazelix_zellij_pane_orchestrator.wasm \
              "$out/share/yazelix_zellij_pane_orchestrator/yazelix_pane_orchestrator.wasm"
            install -Dm644 README.md "$out/share/doc/yazelix_zellij_pane_orchestrator/README.md"

            runHook postInstall
          '';

          doInstallCheck = true;
          nativeInstallCheckInputs = [
            pkgs.coreutils
          ];
          installCheckPhase = ''
            runHook preInstallCheck

            test -s "$out/share/yazelix_zellij_pane_orchestrator/yazelix_pane_orchestrator.wasm"

            runHook postInstallCheck
          '';

          passthru = {
            wasmPath = "share/yazelix_zellij_pane_orchestrator/yazelix_pane_orchestrator.wasm";
          };

          meta = {
            description = "Standalone Zellij pane orchestrator plugin from Yazelix";
            homepage = "https://github.com/luccahuguet/yazelix-zellij-pane-orchestrator";
            license = pkgs.lib.licenses.asl20;
          };
        };
      in
      {
        packages = {
          default = yazelixZellijPaneOrchestrator;
          yazelix-zellij-pane-orchestrator = yazelixZellijPaneOrchestrator;
          yazelix_zellij_pane_orchestrator = yazelixZellijPaneOrchestrator;
        };

        checks = {
          yazelix_zellij_pane_orchestrator = yazelixZellijPaneOrchestrator;
        };

        devShells.default = pkgs.mkShell {
          packages = [
            rustToolchain
          ];
        };
      }
    );
}
