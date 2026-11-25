{
  description = "Air-gapped transaction signer";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
    dev-assets-mkdocs.url = "github:paolino/dev-assets?dir=mkdocs";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils, dev-assets-mkdocs }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        rust = pkgs.rust-bin.stable.latest.default.override {
          targets = [ "wasm32-unknown-unknown" "aarch64-unknown-linux-musl" ];
        };
        # C toolchain for crates with native parts (wasmtime) when
        # cross-compiling the Pi binaries.
        piCc = pkgs.pkgsCross.aarch64-multiplatform-musl.stdenv.cc;
      in {
        devShells.default = pkgs.mkShell ({
          inputsFrom =
            [ dev-assets-mkdocs.devShells.${system}.default ];
          buildInputs = [
            rust
            pkgs.just
            pkgs.wasmtime
            pkgs.python3
            pkgs.file
          ] ++ pkgs.lib.optionals pkgs.stdenv.isLinux [ piCc ];
        } // pkgs.lib.optionalAttrs pkgs.stdenv.isLinux {
          CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER = "${piCc.targetPrefix}cc";
          CC_aarch64_unknown_linux_musl = "${piCc.targetPrefix}cc";
          AR_aarch64_unknown_linux_musl = "${piCc.targetPrefix}ar";
        });
      });
}
