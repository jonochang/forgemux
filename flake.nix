{
  description = "forgemux - durable agent session manager";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
    untangle.url = "github:jonochang/untangle";
    crucible.url = "github:jonochang/crucible";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils, untangle, crucible }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "clippy" "rustfmt" "rust-src" ];
        };
        forgemuxPkg = pkgs.callPackage ./package.nix { };
        untanglePkg = pkgs.callPackage "${untangle}/package.nix" { };
        cruciblePkg = pkgs.callPackage "${crucible}/package.nix" { };
      in
      {
        packages.forgemux = forgemuxPkg;
        packages.default = forgemuxPkg;

        devShells.default = pkgs.mkShell {
          buildInputs = [
            rustToolchain
            untanglePkg
            cruciblePkg

            # Native build dependencies
            pkgs.pkg-config
            pkgs.cmake
            pkgs.openssl
            pkgs.libgit2

            # Cargo dev tools
            pkgs.cargo-nextest
            pkgs.cargo-deny
            pkgs.cargo-llvm-cov
            pkgs.cargo-mutants
            pkgs.cargo-insta

            # Documentation
            pkgs.mdbook

            # Utilities
            pkgs.git
            pkgs.tmux
          ];

          env = {
            LIBGIT2_NO_VENDOR = "1";
            OPENSSL_DIR = "${pkgs.openssl.dev}";
            OPENSSL_LIB_DIR = "${pkgs.openssl.out}/lib";
            OPENSSL_INCLUDE_DIR = "${pkgs.openssl.dev}/include";
          };

          shellHook = ''
            if git rev-parse --git-dir > /dev/null 2>&1; then
              git config core.hooksPath .githooks
            fi
          '';
        };
      }
    );
}
