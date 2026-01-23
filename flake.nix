{
  inputs = {
    nixpkgs.url = "github:cachix/devenv-nixpkgs/rolling";
    crane.url = "github:ipetkov/crane";
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

          # Common arguments shared between deps and main build
          commonArgs = {
            pname = "memex";
            version = "0.1.0";
            src = pkgs.lib.cleanSourceWith {
              src = ./.;
              filter =
                path: type:
                # Include all Cargo/Rust files
                (craneLib.filterCargoSources path type)
                # Also include .surql migration files and .html templates
                || (pkgs.lib.hasSuffix ".surql" path)
                || (pkgs.lib.hasSuffix ".html" path)
                # Include pre-built WASM bundle for embedded web UI
                || (pkgs.lib.hasSuffix ".js" path && pkgs.lib.hasInfix "/web/pkg/" path)
                || (pkgs.lib.hasSuffix ".wasm" path && pkgs.lib.hasInfix "/web/pkg/" path)
                # Include CSS for web UI
                || (pkgs.lib.hasSuffix ".css" path && pkgs.lib.hasInfix "/web/style/" path);
            };
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
          }
          // pkgs.lib.optionalAttrs pkgs.stdenv.isDarwin {
            # Fix C++ standard library include path for RocksDB compilation on macOS
            # NIX_CFLAGS_COMPILE is used by the stdenv cc wrapper
            NIX_CFLAGS_COMPILE = "-isystem ${pkgs.llvmPackages.libcxx}/include/c++/v1";
            # Also need to set it for C++ specifically
            NIX_CXXSTDLIB_COMPILE = "-isystem ${pkgs.llvmPackages.libcxx}/include/c++/v1";
            # Ensure cc-rs finds the stdenv compiler wrapper
            preBuild = ''
              export PATH="${pkgs.stdenv.cc}/bin:$PATH"
            '';
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
