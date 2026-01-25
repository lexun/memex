{
  inputs = {
    nixpkgs.url = "github:cachix/devenv-nixpkgs/rolling";
    crane.url = "github:ipetkov/crane";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
    devenv.url = "github:cachix/devenv";
    devenv.inputs.nixpkgs.follows = "nixpkgs";
  };

  nixConfig = {
    extra-trusted-public-keys = "devenv.cachix.org-1:w1cLUi8dv3hnoSPGAuibQv+f9TZLr6cv/Hm9XgU50cw=";
    extra-substituters = "https://devenv.cachix.org";
  };

  outputs =
    {
      self,
      nixpkgs,
      crane,
      rust-overlay,
      devenv,
      systems,
      ...
    }@inputs:
    let
      devenvEnabled = (builtins.getEnv "DEVENV_ENABLED") == "true";
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forEachSupportedSystem =
        fn:
        nixpkgs.lib.genAttrs supportedSystems (
          system:
          fn {
            inherit system;
            pkgs = import nixpkgs {
              inherit system;
              config.allowUnfree = true;
              overlays = [ rust-overlay.overlays.default ];
            };
          }
        );
    in
    {
      formatter = forEachSupportedSystem ({ pkgs, ... }: pkgs.nixfmt-rfc-style);

      packages = forEachSupportedSystem (
        { pkgs, system, ... }:
        let
          craneLib = crane.mkLib pkgs;

          # Rust toolchain with WASM target for building web UI
          rustToolchainWithWasm = pkgs.rust-bin.stable.latest.default.override {
            targets = [ "wasm32-unknown-unknown" ];
          };
          craneLibWasm = (crane.mkLib pkgs).overrideToolchain rustToolchainWithWasm;

          # Source filtering for the main build
          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter =
              path: type:
              # Include all Cargo/Rust files
              (craneLib.filterCargoSources path type)
              # Also include .surql migration files and .html templates
              || (pkgs.lib.hasSuffix ".surql" path)
              || (pkgs.lib.hasSuffix ".html" path)
              # Include CSS for web UI
              || (pkgs.lib.hasSuffix ".css" path && pkgs.lib.hasInfix "/web/style/" path);
          };

          # Build wasm-bindgen-cli matching the version in Cargo.lock (0.2.106)
          wasmBindgenCli = pkgs.rustPlatform.buildRustPackage rec {
            pname = "wasm-bindgen-cli";
            version = "0.2.106";
            src = pkgs.fetchCrate {
              inherit pname version;
              hash = "sha256-M6WuGl7EruNopHZbqBpucu4RWz44/MSdv6f0zkYw+44=";
            };
            cargoHash = "sha256-ElDatyOwdKwHg3bNH/1pcxKI7LXkhsotlDPQjiLHBwA=";
            doCheck = false;
          };

          # Build WASM bundle for the web UI
          webWasm = craneLibWasm.buildPackage {
            pname = "memex-web-wasm";
            version = "0.1.0";
            src = src;
            cargoExtraArgs = "-p memex-web --features csr";
            CARGO_BUILD_TARGET = "wasm32-unknown-unknown";
            doCheck = false;
            nativeBuildInputs = [ wasmBindgenCli pkgs.binaryen ];
            installPhase = ''
              mkdir -p $out
              wasm-bindgen \
                --target web \
                --out-dir $out \
                target/wasm32-unknown-unknown/release/memex_web.wasm
              wasm-opt -Oz -o $out/memex_web_bg.wasm $out/memex_web_bg.wasm
            '';
          };

          # Common arguments shared between deps and main build
          commonArgs = {
            pname = "memex";
            version = "0.1.0";
            inherit src;
            strictDeps = true;

            buildInputs =
              with pkgs;
              [
                openssl
                pkg-config
              ]
              ++ lib.optionals stdenv.isDarwin [
                # Required for C++ standard library headers (cstdint, etc.)
                llvmPackages.libcxx
                apple-sdk_15
              ];

            nativeBuildInputs =
              with pkgs;
              [
                pkg-config
                # Required for surrealdb-librocksdb-sys bindgen
                llvmPackages.libclang
                # Required for git2's vendored-openssl (via vibetree)
                perl
              ]
              ++ lib.optionals stdenv.isDarwin [
                # Ensure proper C++ compiler with headers is available
                llvmPackages.clang
              ];

            # Set LIBCLANG_PATH for bindgen
            LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";

            # Copy WASM bundle into place before cargo build
            preBuild = ''
              mkdir -p crates/web/pkg
              cp ${webWasm}/memex_web.js crates/web/pkg/
              cp ${webWasm}/memex_web_bg.wasm crates/web/pkg/
            '' + pkgs.lib.optionalString pkgs.stdenv.isDarwin ''
              export PATH="${pkgs.stdenv.cc}/bin:$PATH"
            '';
          }
          // pkgs.lib.optionalAttrs pkgs.stdenv.isDarwin {
            # Fix C++ standard library include path for RocksDB compilation on macOS
            # NIX_CFLAGS_COMPILE is used by the stdenv cc wrapper
            NIX_CFLAGS_COMPILE = "-isystem ${pkgs.llvmPackages.libcxx}/include/c++/v1";
            # Also need to set it for C++ specifically
            NIX_CXXSTDLIB_COMPILE = "-isystem ${pkgs.llvmPackages.libcxx}/include/c++/v1";
          };

          # Build *just* the cargo dependencies so they can be cached
          cargoArtifacts = craneLib.buildDepsOnly commonArgs;

          # Build the actual crate, reusing the dependency artifacts
          memexPackage = craneLib.buildPackage (
            commonArgs
            // {
              inherit cargoArtifacts;
              doCheck = false;
            }
          );
        in
        {
          default = memexPackage;
          memex = memexPackage;
        }
      );

      overlays.default = final: prev: {
        memex = self.packages.${final.stdenv.hostPlatform.system}.default;
      };

      devShells = forEachSupportedSystem (
        { pkgs, ... }:
        pkgs.lib.optionalAttrs devenvEnabled {
          default = devenv.lib.mkShell {
            inherit inputs pkgs;
            modules = [
              {
                languages.rust.enable = true;
                packages = with pkgs; [
                  just
                  git
                  surrealdb
                  zellij
                  wasm-pack
                  wasm-bindgen-cli
                  binaryen # for wasm-opt
                  lld # for WASM linking
                ];
                # Fix C++ standard library include path for RocksDB compilation
                # The default libcxx include path is missing the c++/v1 subdirectory
                env.CXXFLAGS = "-isystem ${pkgs.llvmPackages.libcxx}/include/c++/v1";
                git-hooks.hooks.single-line-commit = {
                  enable = true;
                  name = "single-line commit";
                  entry = "bash -c 'test $(grep -cv \"^#\" \"$1\") -le 1' --";
                  stages = [ "commit-msg" ];
                };
              }
            ];
          };
        }
      );
    };
}
