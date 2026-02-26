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
          targets = [ "wasm32-unknown-unknown" ];
        };
      in {
        devShells.default = pkgs.mkShell {
          inputsFrom =
            [ dev-assets-mkdocs.devShells.${system}.default ];
          buildInputs = [
            rust
            pkgs.just
            pkgs.wasmtime
          ];
        };
      });
}
