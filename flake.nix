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
        zellijPluginWasmPackageContract = {
          schemaVersion = 1;
          pluginName = "yazelix-zellij-pane-orchestrator";
          packageAttr = "yazelix_zellij_pane_orchestrator";
          wasmPath = "share/yazelix_zellij_pane_orchestrator/yazelix_pane_orchestrator.wasm";
          wasmTarget = "wasm32-wasip1";
          cargoAuditableDisabled = true;
          cargoBuildHookDisabled = true;
          preBuildPreservesNixRustToolchain = true;
          wasmTargetRustcEnvPinned = true;
          cargoBuildSerialized = true;
          installCheckVerifiesWasm = true;
        };
        yazelixZellijPaneOrchestrator = rustPlatform.buildRustPackage {
          pname = "yazelix-zellij-pane-orchestrator";
          version = "0.1.0";
          src = pkgs.lib.cleanSource ./.;
          cargoLock.lockFile = ./Cargo.lock;
          auditable = !zellijPluginWasmPackageContract.cargoAuditableDisabled;
          dontCargoBuild = zellijPluginWasmPackageContract.cargoBuildHookDisabled;
          doCheck = false;

          buildPhase = ''
            yazelix_saved_cargo="''${CARGO:-$(command -v cargo || true)}"
            yazelix_saved_rustc="''${RUSTC:-$(command -v rustc || true)}"
            yazelix_saved_path="$PATH"
            if [ -z "$yazelix_saved_cargo" ] || [ -z "$yazelix_saved_rustc" ]; then
              echo "Nix Rust hooks did not provide cargo/rustc before preBuild" >&2
              exit 1
            fi

            runHook preBuild

            export CARGO="$yazelix_saved_cargo"
            export RUSTC="$yazelix_saved_rustc"
            export PATH="$yazelix_saved_path"
            export CARGO_BUILD_RUSTC="$RUSTC"
            export CARGO_TARGET_WASM32_WASIP1_RUSTC="$RUSTC"

            wasm_target_libdir="$("$RUSTC" --print target-libdir --target ${zellijPluginWasmPackageContract.wasmTarget})"
            if [ ! -d "$wasm_target_libdir" ]; then
              echo "Rust toolchain is missing wasm32-wasip1 std at $wasm_target_libdir" >&2
              exit 1
            fi

            "$CARGO" build \
              -j 1 \
              --target-dir target \
              --offline \
              --profile release \
              --target ${zellijPluginWasmPackageContract.wasmTarget}

            runHook postBuild
          '';

          installPhase = ''
            runHook preInstall

            install -Dm644 \
              target/wasm32-wasip1/release/yazelix_zellij_pane_orchestrator.wasm \
              "$out/${zellijPluginWasmPackageContract.wasmPath}"
            install -Dm644 README.md "$out/share/doc/yazelix_zellij_pane_orchestrator/README.md"

            runHook postInstall
          '';

          doInstallCheck = true;
          nativeInstallCheckInputs = [
            pkgs.coreutils
          ];
          installCheckPhase = ''
            runHook preInstallCheck

            test -s "$out/${zellijPluginWasmPackageContract.wasmPath}"

            runHook postInstallCheck
          '';

          passthru = {
            inherit zellijPluginWasmPackageContract;
            wasmPath = zellijPluginWasmPackageContract.wasmPath;
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
