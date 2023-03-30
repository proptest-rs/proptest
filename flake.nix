{
  description = "Flake providing proptest development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay, ... } @ inputs:
    flake-utils.lib.eachDefaultSystem
      (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ (import rust-overlay) ];
          };
        in
        {
          devShells.default = pkgs.mkShell {
            nativeBuildInputs = with pkgs; [
              rust-bin.stable.latest.default

              cargo-insta
            ];
          };
        }
      );
}
